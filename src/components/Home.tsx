import { useCallback, useEffect, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { ask, open as openDialog } from "@tauri-apps/plugin-dialog";
import { api, errorMessage } from "../lib/api";
import { ago, clock } from "../lib/format";
import { hasTranslatorKey, type ProjectSummary, type ProjectView, type Status } from "../lib/types";

const VIDEO_EXTENSIONS = ["mp4", "mov", "m4v", "mkv", "webm", "avi"];

const STAGE_LABEL: Record<ProjectSummary["stage"], string> = {
  new: "Not captioned yet",
  transcribed: "Transcribed",
  captioned: "Captions ready",
  exported: "Exported",
};

interface Props {
  status: Status | null;
  statusError: string | null;
  /** `fresh` when the video was just added, so captioning can start at once. */
  onOpen: (view: ProjectView, fresh?: boolean) => void;
  onOpenSettings: () => void;
}

export function Home({ status, statusError, onOpen, onOpenSettings }: Props) {
  const [projects, setProjects] = useState<ProjectSummary[] | null>(null);
  const [importing, setImporting] = useState<{ name: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setProjects(await api.listProjects());
    } catch (e) {
      setError(errorMessage(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const importVideo = useCallback(
    async (path: string) => {
      setError(null);
      setImporting({ name: path.split(/[\\/]/).pop() ?? path });
      try {
        onOpen(await api.importVideo(path), true);
      } catch (e) {
        setError(errorMessage(e));
      } finally {
        setImporting(null);
      }
    },
    [onOpen],
  );

  useEffect(() => {
    const unlisten = getCurrentWebview().onDragDropEvent((event) => {
      const payload = event.payload;
      if (payload.type === "enter" || payload.type === "over") setDragging(true);
      else if (payload.type === "leave") setDragging(false);
      else if (payload.type === "drop") {
        setDragging(false);
        const path = payload.paths[0];
        if (path) void importVideo(path);
      }
    });
    return () => void unlisten.then((f) => f());
  }, [importVideo]);

  async function choose() {
    const picked = await openDialog({ multiple: false, filters: [{ name: "Video", extensions: VIDEO_EXTENSIONS }] });
    if (typeof picked === "string") void importVideo(picked);
  }

  async function openProject(id: string) {
    setError(null);
    try {
      onOpen(await api.openProject(id));
    } catch (e) {
      setError(errorMessage(e));
    }
  }

  async function removeProject(p: ProjectSummary) {
    const yes = await ask(`Remove "${p.name}" and its captions? The original video is not touched.`, {
      title: "Remove project",
      kind: "warning",
    });
    if (!yes) return;
    try {
      await api.deleteProject(p.id);
      await refresh();
    } catch (e) {
      setError(errorMessage(e));
    }
  }

  const missingKeys = status && (!status.has_elevenlabs_key || !hasTranslatorKey(status));

  return (
    <main className="home">
      <header className="home-header">
        <div>
          <h1>Subtitles</h1>
          <p className="muted">Hindi and Hinglish speech to burned-in English captions, without losing video quality.</p>
        </div>
        <button className="ghost" onClick={onOpenSettings}>
          Settings
        </button>
      </header>

      {statusError && <div className="banner error">{statusError}</div>}
      {status && !status.tools.ok && (
        <div className="banner error">
          This ffmpeg can't be used for exports. Missing: {status.tools.missing.join(", ")}.
        </div>
      )}
      {missingKeys && (
        <div className="banner">
          <span>Add your API keys in Settings to generate captions.</span>
          <button onClick={onOpenSettings}>Add keys</button>
        </div>
      )}
      {error && <div className="banner error">{error}</div>}

      <button className={`dropzone${dragging ? " dragging" : ""}`} onClick={choose} disabled={importing !== null}>
        {importing ? (
          <>
            <strong>Opening {importing.name}</strong>
            <Progress fraction={null} />
          </>
        ) : (
          <>
            <strong>Open a video</strong>
            <span className="muted">or drop one here</span>
          </>
        )}
      </button>

      {projects && projects.length > 0 && (
        <section>
          <h2>Your videos</h2>
          <ul className="project-list">
            {projects.map((p) => (
              <li key={p.id}>
                <button className="project" onClick={() => openProject(p.id)}>
                  <span className={`thumb${p.width < p.height ? " portrait" : ""}`}>
                    {p.thumb_url ? <img src={p.thumb_url} alt="" loading="lazy" /> : <span aria-hidden>▶</span>}
                  </span>
                  <span className="project-text">
                    <span className="project-name">{p.name}</span>
                    <span className="muted small">
                      {clock(p.duration, 0)} · {p.width < p.height ? "vertical" : "horizontal"} · {ago(p.updated_at)}
                    </span>
                    <span className="project-tags">
                      <span className={`tag stage-${p.stage}`}>{STAGE_LABEL[p.stage]}</span>
                      {p.source_missing && <span className="tag warn">Original file missing</span>}
                    </span>
                  </span>
                </button>
                <button
                  className="ghost small project-remove"
                  onClick={() => removeProject(p)}
                  aria-label={`Remove ${p.name}`}
                  title="Remove from this list (the original video is not touched)"
                >
                  Remove
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}
    </main>
  );
}

export function Progress({ fraction }: { fraction: number | null }) {
  return (
    <div className={`progress${fraction === null ? " indeterminate" : ""}`} role="progressbar">
      <div style={fraction === null ? undefined : { width: `${Math.round(fraction * 100)}%` }} />
    </div>
  );
}
