// Line breaking for captions. The preview and the export both use the breaks
// computed here, so what is reviewed is exactly what gets burned in.

import { SIDE_MARGIN_PCT, emPx } from "./style";
import type { Caption, Style } from "./types";

/** Returns the width of `text`, in the same unit as the `maxWidth` it is compared with. */
export type Measure = (text: string) => number;

/**
 * Breaks `text` into the fewest lines that fit `maxWidth`, then balances them
 * so the widest line is as narrow as possible. A single word wider than
 * `maxWidth` gets a line of its own.
 */
export function wrapLines(text: string, maxWidth: number, measure: Measure): string[] {
  const words = text.trim().split(/\s+/).filter(Boolean);
  if (words.length === 0) return [];

  const width = (from: number, to: number) => measure(words.slice(from, to).join(" "));

  // Fewest lines: fill each line greedily.
  let lineCount = 1;
  let lineStart = 0;
  for (let i = 1; i < words.length; i++) {
    if (width(lineStart, i + 1) > maxWidth) {
      lineCount++;
      lineStart = i;
    }
  }
  if (lineCount === 1) return [words.join(" ")];

  // best[l][i]: smallest possible widest-line when words[0..i) fill l lines.
  const n = words.length;
  const best: number[][] = Array.from({ length: lineCount + 1 }, () => new Array<number>(n + 1).fill(Infinity));
  const cut: number[][] = Array.from({ length: lineCount + 1 }, () => new Array<number>(n + 1).fill(0));
  best[0][0] = 0;
  for (let l = 1; l <= lineCount; l++) {
    for (let i = l; i <= n; i++) {
      for (let j = l - 1; j < i; j++) {
        const widest = Math.max(best[l - 1][j], width(j, i));
        if (widest < best[l][i]) {
          best[l][i] = widest;
          cut[l][i] = j;
        }
      }
    }
  }

  const lines: string[] = [];
  for (let l = lineCount, end = n; l > 0; l--) {
    const start = cut[l][end];
    lines.unshift(words.slice(start, end).join(" "));
    end = start;
  }
  return lines;
}

let canvas: CanvasRenderingContext2D | null = null;
const REFERENCE_PX = 100;

/** Measures caption text in em, using the caption font loaded by the page. */
export const measureEm: Measure = (text) => {
  canvas ??= document.createElement("canvas").getContext("2d");
  if (!canvas) return text.length * 0.66;
  canvas.font = `800 ${REFERENCE_PX}px "Caption"`;
  return canvas.measureText(text).width / REFERENCE_PX;
};

/** Resolves once the caption font can be measured. */
export function captionFontReady(): Promise<unknown> {
  return document.fonts.load(`800 ${REFERENCE_PX}px "Caption"`);
}

/** The widest a caption line may be, in em, for a video of this shape. */
export function maxLineEm(width: number, height: number, style: Style): number {
  const usablePx = width * (1 - (2 * SIDE_MARGIN_PCT) / 100);
  return usablePx / emPx(width, height, style);
}

/** Each caption's text with `\n` at its line breaks, in caption order. */
export function wrapAll(captions: Caption[], width: number, height: number, style: Style, measure: Measure = measureEm): string[] {
  const limit = maxLineEm(width, height, style);
  return captions.map((c) => wrapLines(c.english, limit, measure).join("\n"));
}
