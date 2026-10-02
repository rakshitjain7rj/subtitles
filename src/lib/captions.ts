// Pure editing operations on the caption list. Each returns a new array and
// leaves its input untouched, so the editor can keep an undo history.

import type { Caption } from "./types";

/** Shortest a caption may be made by nudging or splitting, in seconds. */
export const MIN_DURATION = 0.1;
export const NUDGE_STEP = 0.05;

const round = (t: number) => Math.round(t * 1000) / 1000;

function newId(): string {
  return crypto.randomUUID();
}

/** Index of the caption on screen at time `t`, or -1. */
export function activeIndex(captions: Caption[], t: number): number {
  return captions.findIndex((c) => t >= c.start && t < c.end);
}

export function setText(captions: Caption[], i: number, english: string): Caption[] {
  return captions.map((c, j) => (j === i ? { ...c, english } : c));
}

/**
 * Moves one edge of a caption by `delta` seconds. An edge pushed into a
 * neighbour moves the neighbour's edge with it, so captions never overlap.
 */
export function nudge(captions: Caption[], i: number, edge: "start" | "end", delta: number): Caption[] {
  const out = captions.map((c) => ({ ...c }));
  const c = out[i];
  if (!c) return captions;
  if (edge === "start") {
    const prev = out[i - 1];
    const floor = prev ? prev.start + MIN_DURATION : 0;
    c.start = round(Math.min(Math.max(c.start + delta, floor), c.end - MIN_DURATION));
    if (prev && prev.end > c.start) prev.end = c.start;
  } else {
    const next = out[i + 1];
    const ceiling = next ? next.end - MIN_DURATION : Infinity;
    c.end = round(Math.max(Math.min(c.end + delta, ceiling), c.start + MIN_DURATION));
    if (next && next.start < c.end) next.start = c.end;
  }
  return out;
}

/** Splits `text` into two halves at the word boundary nearest `at` (a character offset). */
function splitWords(text: string, at: number): [string, string] | null {
  const words = text.trim().split(/\s+/).filter(Boolean);
  if (words.length < 2) return null;
  let best = 1;
  let bestDistance = Infinity;
  let offset = 0;
  for (let k = 1; k < words.length; k++) {
    offset += words[k - 1].length + 1;
    const distance = Math.abs(offset - at);
    if (distance < bestDistance) {
      best = k;
      bestDistance = distance;
    }
  }
  return [words.slice(0, best).join(" "), words.slice(best).join(" ")];
}

/**
 * Splits a caption in two at the word boundary nearest `caret` (default: the
 * middle). Time is divided in proportion to the text on each side.
 */
export function split(captions: Caption[], i: number, caret?: number): Caption[] {
  const c = captions[i];
  if (!c) return captions;
  const text = c.english.trim();
  const halves = splitWords(text, caret ?? text.length / 2);
  if (!halves || c.end - c.start < 2 * MIN_DURATION) return captions;
  const [left, right] = halves;
  const ratio = left.length / (left.length + right.length);
  const lo = c.start + MIN_DURATION;
  const hi = c.end - MIN_DURATION;
  const mid = round(Math.min(Math.max(c.start + (c.end - c.start) * ratio, lo), hi));

  const hindiWords = c.hindi.split(/\s+/).filter(Boolean);
  const hindiCut = Math.round(hindiWords.length * ratio);
  const first: Caption = { ...c, end: mid, english: left, hindi: hindiWords.slice(0, hindiCut).join(" ") };
  const second: Caption = { id: newId(), start: mid, end: c.end, english: right, hindi: hindiWords.slice(hindiCut).join(" ") };
  return [...captions.slice(0, i), first, second, ...captions.slice(i + 1)];
}

/** Merges caption `i` with the one after it. */
export function mergeWithNext(captions: Caption[], i: number): Caption[] {
  const a = captions[i];
  const b = captions[i + 1];
  if (!a || !b) return captions;
  const join = (x: string, y: string) => [x.trim(), y.trim()].filter(Boolean).join(" ");
  const merged: Caption = { ...a, end: b.end, english: join(a.english, b.english), hindi: join(a.hindi, b.hindi) };
  return [...captions.slice(0, i), merged, ...captions.slice(i + 2)];
}

export function remove(captions: Caption[], i: number): Caption[] {
  return captions.filter((_, j) => j !== i);
}

/**
 * Adds an empty caption starting at `t`, up to one second long, in the gap
 * between existing captions. Returns null when `t` has no room for one.
 */
export function insertAt(captions: Caption[], t: number, duration: number): { captions: Caption[]; index: number } | null {
  if (activeIndex(captions, t) !== -1) return null;
  const index = captions.findIndex((c) => c.start > t);
  const at = index === -1 ? captions.length : index;
  const limit = Math.min(captions[at]?.start ?? duration, duration);
  const start = round(Math.max(0, t));
  const end = round(Math.min(start + 1, limit));
  if (end - start < MIN_DURATION) return null;
  const added: Caption = { id: newId(), start, end, english: "", hindi: "" };
  return { captions: [...captions.slice(0, at), added, ...captions.slice(at)], index: at };
}
