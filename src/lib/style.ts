// The caption look, mirroring `src-tauri/src/ass.rs` so the preview matches
// what ffmpeg burns in. Change the numbers in both places together.

import type { CSSProperties } from "react";
import type { Style } from "./types";

export const LINE_HEIGHT = 1.562;
const OUTLINE_PER_EM = 0.09;
export const SIDE_MARGIN_PCT = 6;

export const STYLE_LIMITS = { y: [5, 95], size: [2.5, 14] } as const;
export const DEFAULT_STYLE: Style = { y_pct: 72, size_pct: 6.5 };

/** Text size in source-video pixels. */
export function emPx(width: number, height: number, style: Style): number {
  return Math.max(8, Math.round((style.size_pct / 100) * Math.min(width, height)));
}

/** A ring of shadows: the closest CSS gets to libass's rounded outline. */
function outlineShadow(radius: number): string {
  const shadows: string[] = [];
  for (const [r, steps] of [[radius, 24], [radius * 0.5, 12]] as const) {
    for (let k = 0; k < steps; k++) {
      const angle = (2 * Math.PI * k) / steps;
      shadows.push(`${(Math.cos(angle) * r).toFixed(2)}px ${(Math.sin(angle) * r).toFixed(2)}px 0 #000`);
    }
  }
  return shadows.join(",");
}

/**
 * CSS for the caption, in source-video pixels. The preview lays it out at
 * full size and scales the whole layer down, so text metrics are the same as
 * in the export rather than re-rounded at the preview's smaller size.
 */
export function overlayCss(width: number, height: number, style: Style): CSSProperties {
  const em = emPx(width, height, style);
  return {
    top: `${style.y_pct}%`,
    left: `${SIDE_MARGIN_PCT}%`,
    right: `${SIDE_MARGIN_PCT}%`,
    fontSize: `${em}px`,
    lineHeight: LINE_HEIGHT,
    textShadow: outlineShadow(em * OUTLINE_PER_EM),
  };
}
