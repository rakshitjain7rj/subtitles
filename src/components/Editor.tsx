import { useCallback, useEffect, useRef, useState } from "react";
import { ask, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { api, errorMessage, onExportMeasured, onProgress } from "../lib/api";
import { insertAt, mergeWithNext, nudge, remove, retime, setText, split } from "../lib/captions";
import { describeVideo, estimateCostUsd, usd } from "../lib/format";
import { STYLE_LIMITS } from "../lib/style";
import { captionFontReady, wrapAll } from "../lib/wrap";
import { hasTranslatorKey, type Caption, type ProjectView, type Stage, type Status, type Style } from "../lib/types";
import { CaptionList, type CaptionAction } from "./CaptionList";
import { ExportReport } from "./ExportReport";
import { Progress } from "./Home";
import { Timeline } from "./Timeline";
import { VideoPreview, type PreviewHandle } from "./VideoPreview";

const STAGE_LABEL: Record<Stage, string> = {
  preview: "Preparing preview",
  audio: "Extracting audio",
  transcribe: "Transcribing speech",
  translate: "Translating to English",
  encode: "Encoding video",
  verify: "Measuring quality against the original",
};

/** Caption heights to pick from; the slider fine-tunes. `y` is % from the top. */
const POSITIONS = [
  { label: "Top", y: 18 },
  { label: "Middle", y: 50 },
  { label: "Reels-safe", y: 72, hint: "Low, but above Instagram and YouTube's buttons" },
  { label: "Bottom", y: 86 },
];
const SIZES = [
  { label: "S", size: 5 },
  { label: "M", size: 6.5 },
  { label: "L", size: 8 },
];

const SAVE_DELAY_MS = 500;
const UNDO_LIMIT = 100;
const TYPING_PAUSE_MS = 1500;

interface Props {
  initial: ProjectView;
  /** Just added: start captioning at once instead of waiting for a click. */
  fresh: boolean;
  status: Status | null;
  onBack: () => void;
  onOpenSettings: () => void;
}

interface Busy {
  stage: Stage;
  fraction: number | null;
}

export function Editor({ initial, fresh, status, onBack, onOpenSettings }: Props) {
  const [view, setView] = useState(initial);
  const [captions, setCaptions] = useState<Caption[]>(initial.project.captions);
  const [style, setStyle] = useState<Style>(initial.project.style);
  const [selected, setSelected] = useState(-1);
  const [playing, setPlaying] = useState(-1);
  const [busy, setBusy] = useState<Busy | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [showReport, setShowReport] = useState(false);
  const [saveState, setSaveState] = useState<"saved" | "saving" | "failed">("saved");
  // The preview and the quality check run alongside other work, so they have
  // their own progress rather than the single `busy` banner.
  const [previewReady, setPreviewReady] = useState(initial.preview_ready);
  const [previewFraction, setPreviewFraction] = useState<number | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [measuring, setMeasuring] = useState<number | null | false>(false);

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

  const selectedRef = useRef(selected);
  selectedRef.current = selected;

  const select = useCallback((index: number) => {
    setSelected(index);
    const c = latest.current.captions[index];
    // Land just inside the caption so it is the one shown.
    if (c) preview.current?.seek(c.start + 0.001);
  }, []);

  // Timeline drags: one undo step per drag, then live updates as it moves.
  const beginRetime = useCallback(() => {
    undoStack.current.push(latest.current.captions);
    if (undoStack.current.length > UNDO_LIMIT) undoStack.current.shift();
    typingIn.current = null;
  }, []);

  const retimeTo = useCallback(
    (index: number, edge: "start" | "end", t: number) => {
      const current = latest.current.captions;
      const next = retime(current, index, edge, t, info.duration);
      if (next === current) return;
      setCaptions(next);
      markDirty();
    },
    [info.duration, markDirty],
  );

  const getTime = useCallback(() => preview.current?.time() ?? 0, []);
  const seekTo = useCallback((t: number) => preview.current?.seek(t), []);

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
      } else if ((e.key === "ArrowDown" || e.key === "ArrowUp") && !typing) {
        e.preventDefault();
        const count = latest.current.captions.length;
        if (count === 0) return;
        const from = selectedRef.current;
        const to = e.key === "ArrowDown" ? Math.min(from + 1, count - 1) : Math.max(from - 1, 0);
        select(from === -1 ? 0 : to);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [undo, select]);

  // --- long-running steps -------------------------------------------------

  useEffect(() => {
    const unlisten = onProgress((p) => {
      if (p.project_id !== id) return;
      if (p.stage === "preview") setPreviewFraction(p.fraction);
      else if (p.stage === "verify") setMeasuring((cur) => (cur === false ? cur : p.fraction));
      else setBusy((cur) => (cur ? { stage: p.stage, fraction: p.fraction } : cur));
    });
    return () => void unlisten.then((f) => f());
  }, [id]);

  useEffect(() => {
    if (previewReady) return;
    api.buildPreview(id).then(
      () => setPreviewReady(true),
      (e) => setPreviewError(errorMessage(e)),
    );
    // Only on opening; the preview is built once.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);

  // The score arrives after the file is written; take only the export record,
  // so edits made meanwhile are kept.
  useEffect(() => {
    const unlisten = onExportMeasured((next) => {
      if (next.project.id !== id) return;
      setView((cur) => ({ ...cur, project: { ...cur.project, last_export: next.project.last_export } }));
      setMeasuring(false);
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

  // A new video starts captioning straight away, once, while the preview is made.
  const autoStarted = useRef(false);
  useEffect(() => {
    if (!fresh || autoStarted.current || !keysReady || hasTranscript || captions.length > 0) return;
    autoStarted.current = true;
    void generate();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [fresh, keysReady]);

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
    if (exported) {
      setMeasuring(null);
      setShowReport(true);
    }
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
        <Steps hasCaptions={hasCaptions} exported={project.last_export !== null} />
        <span className={`muted small save-state ${saveState}`}>
          {saveState === "saved" ? (hasCaptions ? "Saved" : "") : saveState === "saving" ? "Saving…" : "Not saved"}
        </span>
        {project.last_export && (
          <button className="ghost" onClick={() => setShowReport(true)} disabled={busy !== null}>
            Last export
          </button>
        )}
        <button
          className="primary"
          onClick={exportVideo}
          disabled={busy !== null || !hasCaptions || measuring !== false}
          title={measuring !== false ? "Measuring the last export's quality" : undefined}
        >
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
            url={previewReady ? view.preview_url : null}
            preparing={previewError ?? previewFraction}
            info={info}
            captions={captions}
            style={style}
            onActiveChange={setPlaying}
          />
          <div className="style-controls">
            <div className="style-row">
              <span className="style-label">Position</span>
              <div className="segmented" role="group" aria-label="Caption position">
                {POSITIONS.map((p) => (
                  <button
                    key={p.label}
                    className={Math.abs(style.y_pct - p.y) < 0.75 ? "on" : ""}
                    title={p.hint}
                    onClick={() => changeStyle({ y_pct: p.y })}
                  >
                    {p.label}
                  </button>
                ))}
              </div>
              <input
                type="range"
                aria-label="Fine-tune position"
                min={STYLE_LIMITS.y[0]}
                max={STYLE_LIMITS.y[1]}
                step={0.5}
                value={style.y_pct}
                onChange={(e) => changeStyle({ y_pct: Number(e.target.value) })}
              />
            </div>
            <div className="style-row">
              <span className="style-label">Size</span>
              <div className="segmented" role="group" aria-label="Caption size">
                {SIZES.map((z) => (
                  <button
                    key={z.label}
                    className={Math.abs(style.size_pct - z.size) < 0.2 ? "on" : ""}
                    onClick={() => changeStyle({ size_pct: z.size })}
                  >
                    {z.label}
                  </button>
                ))}
              </div>
              <input
                type="range"
                aria-label="Fine-tune size"
                min={STYLE_LIMITS.size[0]}
                max={STYLE_LIMITS.size[1]}
                step={0.25}
                value={style.size_pct}
                onChange={(e) => changeStyle({ size_pct: Number(e.target.value) })}
              />
            </div>
          </div>
          <p className="muted small shortcuts">
            <kbd>Space</kbd> play/pause · <kbd>↑</kbd> <kbd>↓</kbd> previous/next caption · <kbd>Ctrl</kbd>+<kbd>Z</kbd> undo
          </p>
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
                <span
                  className="muted small list-count"
                  title={project.translated_with ? `Translated with ${project.translated_with}` : undefined}
                >
                  {captions.length} captions
                  {project.translated_with ? ` · ${project.translated_with}` : ""}
                </span>
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
              {busy ? (
                <>
                  <h2>Making captions…</h2>
                  <p className="muted">
                    The speech is being transcribed and translated. A one-minute video usually takes under a minute.
                    You can watch the preview meanwhile.
                  </p>
                </>
              ) : (
                <>
                  <h2>{hasTranscript ? "Translate the transcript" : "Generate captions"}</h2>
                  <p className="muted">
                    {hasTranscript
                      ? "The speech is already transcribed and saved. Translating turns it into short English captions."
                      : "The speech is transcribed with word timings, then translated into short English captions you can review and edit."}
                  </p>
                  {keysReady ? (
                    <>
                      <button className="primary" onClick={generate}>
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
                </>
              )}
            </div>
          )}
        </section>
        {hasCaptions && (
          <Timeline
            duration={info.duration}
            captions={captions}
            words={project.words}
            selected={selected}
            getTime={getTime}
            onSeek={seekTo}
            onSelect={select}
            onDragStart={beginRetime}
            onRetime={retimeTo}
          />
        )}
      </div>

      {showReport && project.last_export && <ExportReport record={project.last_export} measuring={measuring} onClose={() => setShowReport(false)} />}
    </div>
  );
}

/** Where this video is in the three steps from video to captioned file. */
function Steps({ hasCaptions, exported }: { hasCaptions: boolean; exported: boolean }) {
  const steps = [
    { label: "Captions", done: hasCaptions },
    { label: "Review", done: exported },
    { label: "Export", done: exported },
  ];
  const current = steps.findIndex((st) => !st.done);
  return (
    <ol className="steps" aria-label="Progress">
      {steps.map((st, i) => (
        <li key={st.label} className={st.done ? "done" : i === current ? "current" : ""}>
          <span className="step-dot">{st.done ? "✓" : i + 1}</span>
          <span className="step-label">{st.label}</span>
        </li>
      ))}
    </ol>
  );
}
