import { useCallback, useEffect, useRef, useState } from "react";
import { ask, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { api, errorMessage, onProgress } from "../lib/api";
import { insertAt, mergeWithNext, nudge, remove, setText, split } from "../lib/captions";
import { describeVideo, estimateCostUsd, usd } from "../lib/format";
import { STYLE_LIMITS } from "../lib/style";
import { captionFontReady, wrapAll } from "../lib/wrap";
import { hasTranslatorKey, type Caption, type ProjectView, type Stage, type Status, type Style } from "../lib/types";
import { CaptionList, type CaptionAction } from "./CaptionList";
import { ExportReport } from "./ExportReport";
import { Progress } from "./Home";
import { VideoPreview, type PreviewHandle } from "./VideoPreview";

const STAGE_LABEL: Record<Stage, string> = {
  preview: "Preparing preview",
  audio: "Extracting audio",
  transcribe: "Transcribing speech",
  translate: "Translating to English",
  encode: "Encoding video",
  verify: "Measuring quality against the original",
};

const SAVE_DELAY_MS = 500;
const UNDO_LIMIT = 100;
const TYPING_PAUSE_MS = 1500;

interface Props {
  initial: ProjectView;
  status: Status | null;
  onBack: () => void;
  onOpenSettings: () => void;
}

interface Busy {
  stage: Stage;
  fraction: number | null;
}

export function Editor({ initial, status, onBack, onOpenSettings }: Props) {
  const [view, setView] = useState(initial);
  const [captions, setCaptions] = useState<Caption[]>(initial.project.captions);
  const [style, setStyle] = useState<Style>(initial.project.style);
  const [selected, setSelected] = useState(-1);
  const [playing, setPlaying] = useState(-1);
  const [busy, setBusy] = useState<Busy | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [showReport, setShowReport] = useState(false);
  const [saveState, setSaveState] = useState<"saved" | "saving" | "failed">("saved");

  const preview = useRef<PreviewHandle>(null);
  const undoStack = useRef<Caption[][]>([]);
  const project = view.project;
  const id = project.id;
  const { info } = project;

  // --- saving -------------------------------------------------------------

  const latest = useRef({ captions, style });
  latest.current = { captions, style };
  const dirty = useRef(false);
  const timer = useRef<number | undefined>(undefined);

  const flush = useCallback(async () => {
    window.clearTimeout(timer.current);
    if (!dirty.current) return;
    dirty.current = false;
    setSaveState("saving");
    try {
      await api.saveEdits(id, latest.current.captions, latest.current.style);
      if (!dirty.current) setSaveState("saved");
    } catch (e) {
      dirty.current = true;
      setSaveState("failed");
      throw e;
    }
  }, [id]);

  const markDirty = useCallback(() => {
    dirty.current = true;
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => void flush().catch(() => {}), SAVE_DELAY_MS);
  }, [flush]);

  // Don't lose the last half-second of edits when leaving the editor.
  useEffect(() => () => void flush().catch(() => {}), [flush]);

  // --- editing ------------------------------------------------------------

  // A run of keystrokes in one caption is a single undo step.
  const typingIn = useRef<{ index: number; at: number } | null>(null);

  const edit = useCallback(
    (next: Caption[], current: Caption[], typedIndex?: number) => {
      if (next === current) return;
      const last = typingIn.current;
      const continuesTyping = typedIndex !== undefined && last?.index === typedIndex && Date.now() - last.at < TYPING_PAUSE_MS;
      if (!continuesTyping) {
        undoStack.current.push(current);
        if (undoStack.current.length > UNDO_LIMIT) undoStack.current.shift();
      }
      typingIn.current = typedIndex === undefined ? null : { index: typedIndex, at: Date.now() };
      setCaptions(next);
      markDirty();
    },
    [markDirty],
  );

  const onAction = useCallback(
    (action: CaptionAction) => {
      const current = latest.current.captions;
      switch (action.type) {
        case "text":
          return edit(setText(current, action.index, action.english), current, action.index);
        case "nudge":
          return edit(nudge(current, action.index, action.edge, action.delta), current);
        case "split":
          return edit(split(current, action.index, action.caret), current);
        case "merge":
          return edit(mergeWithNext(current, action.index), current);
        case "remove":
          setSelected(-1);
          return edit(remove(current, action.index), current);
      }
    },
    [edit],
  );

  const undo = useCallback(() => {
    const previous = undoStack.current.pop();
    if (!previous) return;
    setCaptions(previous);
    markDirty();
  }, [markDirty]);

  const select = useCallback((index: number) => {
    setSelected(index);
    const c = latest.current.captions[index];
    // Land just inside the caption so it is the one shown.
    if (c) preview.current?.seek(c.start + 0.001);
  }, []);

  function addCaption() {
    const t = preview.current?.time() ?? 0;
    const added = insertAt(captions, t, project.info.duration);
    if (!added) {
      setError("There is already a caption at this point. Move the playhead to a gap first.");
      return;
    }
    setError(null);
    edit(added.captions, captions);
    setSelected(added.index);
  }

  function changeStyle(patch: Partial<Style>) {
    setStyle((s) => ({ ...s, ...patch }));
    markDirty();
  }

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const typing = e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement;
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "z" && !e.shiftKey) {
        e.preventDefault();
        undo();
      } else if (e.key === " " && !typing && !(e.target instanceof HTMLButtonElement)) {
        e.preventDefault();
        preview.current?.toggle();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [undo]);

  // --- long-running steps -------------------------------------------------

  useEffect(() => {
    const unlisten = onProgress((p) => {
      if (p.project_id === id) setBusy((cur) => (cur ? { stage: p.stage, fraction: p.fraction } : cur));
    });
    return () => void unlisten.then((f) => f());
  }, [id]);

  function adopt(next: ProjectView) {
    setView(next);
    setCaptions(next.project.captions);
    setStyle(next.project.style);
    undoStack.current = [];
    setSelected(-1);
  }

  async function run(first: Stage, work: () => Promise<ProjectView>): Promise<boolean> {
    setError(null);
    setBusy({ stage: first, fraction: null });
    try {
      await flush();
      adopt(await work());
      return true;
    } catch (e) {
      setError(errorMessage(e));
      // A step can fail partway (transcribed, then translation failed), so
      // show whatever did get saved.
      await api.openProject(id).then(adopt, () => {});
      return false;
    } finally {
      setBusy(null);
    }
  }

  const hasTranscript = project.words !== null;
  const translator = status?.settings.translator ?? "gemini";
  const translatorName = translator === "gemini" ? "Gemini" : "Claude";
  const keysReady = !!status && hasTranslatorKey(status) && (hasTranscript || status.has_elevenlabs_key);

  async function generate() {
    await run(hasTranscript ? "translate" : "audio", async () => {
      if (!hasTranscript) await api.transcribe(id, false);
      return api.translate(id);
    });
  }

  async function retranslate() {
    const yes = await ask(
      `Translate again with ${translatorName} from the saved transcript? This replaces the current captions, including your edits. (Change the translator in Settings.)`,
      {
        title: "Translate again",
        kind: "warning",
      },
    );
    if (yes) await run("translate", () => api.translate(id));
  }

  async function exportVideo() {
    let target: string | null;
    try {
      const suggested = await api.defaultExportPath(id);
      const ext = suggested.split(".").pop() ?? "mp4";
      target = await saveDialog({ defaultPath: suggested, filters: [{ name: "Video", extensions: [ext] }] });
    } catch (e) {
      setError(errorMessage(e));
      return;
    }
    if (!target) return;
    const path = target;
    const exported = await run("encode", async () => {
      await captionFontReady();
      const { captions, style } = latest.current;
      return api.exportProject(id, path, wrapAll(captions, info.width, info.height, style));
    });
    if (exported) setShowReport(true);
  }

  // --- render -------------------------------------------------------------

  const hasCaptions = captions.length > 0;
  const costUsd = estimateCostUsd(info.duration, { transcribe: !hasTranscript, translate: translator });
  const costNote =
    costUsd === 0
      ? `Translating with ${translatorName} on Google's free tier costs nothing.`
      : `Estimated cost on your API accounts: ${costUsd < 0.01 ? "" : "about "}${usd(costUsd)}${
          translator === "gemini" ? " for transcription; translating with Gemini is free" : ""
        }.`;

  return (
    <div className="editor">
      <header className="editor-header">
        <button className="ghost" onClick={onBack} disabled={busy !== null}>
          ← Videos
        </button>
        <div className="editor-title">
          <h1>{project.name}</h1>
          <span className="muted small">{describeVideo(info)}</span>
        </div>
        <span className={`muted small save-state ${saveState}`}>
          {saveState === "saved" ? (hasCaptions ? "Saved" : "") : saveState === "saving" ? "Saving…" : "Not saved"}
        </span>
        {project.last_export && (
          <button className="ghost" onClick={() => setShowReport(true)} disabled={busy !== null}>
            Last export
          </button>
        )}
        <button className="primary" onClick={exportVideo} disabled={busy !== null || !hasCaptions}>
          Export video
        </button>
      </header>

      {error && (
        <div className="banner error">
          <span className="pre">{error}</span>
          <button className="ghost" onClick={() => setError(null)}>
            Dismiss
          </button>
        </div>
      )}
      {busy && (
        <div className="banner working">
          <span>{STAGE_LABEL[busy.stage]}…</span>
          <Progress fraction={busy.fraction} />
        </div>
      )}

      <div className="editor-body">
        <section className="editor-left">
          <VideoPreview
            ref={preview}
            url={view.preview_url}
            info={info}
            captions={captions}
            style={style}
            onActiveChange={setPlaying}
          />
          <div className="style-controls">
            <label>
              <span>Position</span>
              <input
                type="range"
                min={STYLE_LIMITS.y[0]}
                max={STYLE_LIMITS.y[1]}
                step={0.5}
                value={style.y_pct}
                onChange={(e) => changeStyle({ y_pct: Number(e.target.value) })}
              />
            </label>
            <label>
              <span>Size</span>
              <input
                type="range"
                min={STYLE_LIMITS.size[0]}
                max={STYLE_LIMITS.size[1]}
                step={0.25}
                value={style.size_pct}
                onChange={(e) => changeStyle({ size_pct: Number(e.target.value) })}
              />
            </label>
          </div>
          {info.hdr !== "none" && (
            <p className="muted small">
              HDR video: the export stays HDR (10-bit HEVC). This preview is a simplified SDR copy, so its colours are only
              approximate.
            </p>
          )}
        </section>

        <section className="editor-right">
          {hasCaptions ? (
            <>
              <div className="list-toolbar">
                <span className="muted small">{captions.length} captions
                  {project.translated_with ? ` · translated with ${project.translated_with}` : ""}
                </span>
                <span className="spacer" />
                <button className="ghost small" onClick={undo} disabled={busy !== null} title="Ctrl+Z">
                  Undo
                </button>
                <button className="ghost small" onClick={addCaption} disabled={busy !== null}>
                  Add caption
                </button>
                <button className="ghost small" onClick={retranslate} disabled={busy !== null || !hasTranscript}>
                  Translate again
                </button>
              </div>
              <CaptionList captions={captions} playing={playing} selected={selected} onSelect={select} onAction={onAction} />
            </>
          ) : (
            <div className="empty">
              <h2>{hasTranscript ? "Translate the transcript" : "Generate captions"}</h2>
              <p className="muted">
                {hasTranscript
                  ? "The speech is already transcribed and saved. Translating turns it into short English captions."
                  : "The speech is transcribed with word timings, then translated into short English captions you can review and edit."}
              </p>
              {keysReady ? (
                <>
                  <button className="primary" onClick={generate} disabled={busy !== null}>
                    {hasTranscript ? "Translate" : "Generate captions"}
                  </button>
                  <p className="muted small">{costNote}</p>
                </>
              ) : (
                <>
                  <button className="primary" onClick={onOpenSettings}>
                    Add API keys
                  </button>
                  <p className="muted small">
                    Needed first: {hasTranscript ? "" : "an ElevenLabs key and "}a {translatorName} key.
                  </p>
                </>
              )}
            </div>
          )}
        </section>
      </div>

      {showReport && project.last_export && <ExportReport record={project.last_export} onClose={() => setShowReport(false)} />}
    </div>
  );
}
