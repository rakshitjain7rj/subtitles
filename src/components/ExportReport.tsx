import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { describeVideo, fileName, fileSize, fps } from "../lib/format";
import type { ExportRecord, QualityReport } from "../lib/types";

type Tone = "good" | "fair" | "poor";

/**
 * What the score means in plain words. With a ceiling (the original scored
 * against itself) the gap to it is what matters; older reports without one
 * fall back to the usual reading that 95 and up is indistinguishable.
 */
export function verdict(mean: number, ceiling: number | null | undefined): { label: string; tone: Tone } {
  if (ceiling != null) {
    const gap = ceiling - mean;
    if (gap < 0.01) return { label: "Identical to the original outside the captions", tone: "good" };
    if (gap <= 0.5) return { label: "Visually identical to the original", tone: "good" };
    if (gap <= 2) return { label: "Very close to the original", tone: "fair" };
    return { label: "Visible differences from the original", tone: "poor" };
  }
  if (mean >= 95) return { label: "Visually identical to the original", tone: "good" };
  if (mean >= 90) return { label: "Very close to the original", tone: "fair" };
  return { label: "Visible differences from the original", tone: "poor" };
}

function checks(r: QualityReport): { label: string; ok: boolean; detail: string }[] {
  const list = [
    { label: "Resolution", ok: r.resolution_match, detail: `${r.export.width}×${r.export.height}` },
    { label: "Frame rate", ok: r.fps_match, detail: `${fps(r.export.fps)} fps` },
    { label: "Audio", ok: r.audio_match, detail: r.export.audio_codec ?? "none" },
    {
      label: "Dynamic range",
      ok: r.hdr_match,
      detail: r.export.hdr === "none" ? "SDR" : `HDR, ${r.export.bit_depth}-bit`,
    },
  ];
  if (r.frame_count_match !== null) {
    list.splice(2, 0, { label: "Frames", ok: r.frame_count_match, detail: `${r.export.frame_count ?? "?"} frames` });
  }
  return list;
}

export function ExportReport({ record, onClose }: { record: ExportRecord; onClose: () => void }) {
  const r = record.report;
  const v = r.vmaf ? verdict(r.vmaf.mean, r.ceiling) : null;
  const sampling = r.frame_step > 1 ? `, every ${r.frame_step}th frame` : "";
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal report" role="dialog" aria-label="Export result">
        <header>
          <h2>Export finished</h2>
          <button className="ghost" onClick={onClose}>
            Close
          </button>
        </header>

        <div className="score">
          {r.vmaf && v ? (
            <>
              <div className={`score-number ${v.tone}`}>
                {r.vmaf.mean.toFixed(1)}
                {r.ceiling != null && <span className="score-of">of {r.ceiling.toFixed(1)}</span>}
              </div>
              <div>
                <strong>{v.label}</strong>
                <p className="muted small">
                  VMAF, comparing the export with the original on the {Math.round(r.measured_fraction * 100)}% of the
                  picture outside the captions{sampling}.{" "}
                  {r.ceiling != null &&
                    `The original compared with itself scores ${r.ceiling.toFixed(1)}: the most this video can reach, since VMAF rarely gives 100 on low-motion video such as screen recordings. `}
                  Lowest frame: {r.vmaf.min.toFixed(1)}.
                  {r.vmaf.psnr_y != null && ` PSNR: ${r.vmaf.psnr_y.toFixed(1)} dB (60 means identical).`}
                </p>
              </div>
            </>
          ) : (
            <strong>No quality score for this export.</strong>
          )}
        </div>

        <ul className="checks">
          {checks(r).map((c) => (
            <li key={c.label} className={c.ok ? "ok" : "bad"}>
              <span>{c.ok ? "✓" : "✕"}</span>
              <span>{c.label}</span>
              <span className="muted">
                {c.detail}
                {c.ok ? ", same as original" : ", differs from original"}
              </span>
            </li>
          ))}
        </ul>

        {r.notes.map((note) => (
          <p key={note} className="muted small">
            {note}
          </p>
        ))}

        {r.quality === "lossless" && (
          <p className="muted small">
            Exported lossless. Some phones and browsers can't play lossless H.264; use High quality for uploading.
          </p>
        )}
        <p className="muted small">
          {fileName(record.path)} · {describeVideo(r.export)} · {fileSize(r.export.size_bytes)} (original{" "}
          {fileSize(r.source.size_bytes)})
        </p>
        <footer>
          <button onClick={() => void revealItemInDir(record.path)}>Show in folder</button>
        </footer>
      </div>
    </div>
  );
}
