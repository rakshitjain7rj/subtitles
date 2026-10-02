import type { MediaInfo } from "./types";

/** 75.3 -> "1:15.3" */
export function clock(secs: number, decimals = 1): string {
  // Round first, so 59.96 reads "1:00.0" rather than "0:60.0".
  const s = Number(Math.max(0, secs).toFixed(decimals));
  const minutes = Math.floor(s / 60);
  const rest = (s - minutes * 60).toFixed(decimals);
  return `${minutes}:${rest.split(".")[0].length < 2 ? "0" : ""}${rest}`;
}

export function fileSize(bytes: number): string {
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(2)} GB`;
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(1)} MB`;
  return `${Math.max(1, Math.round(bytes / 1e3))} KB`;
}

export function fps(value: number): string {
  return Number.isInteger(value) ? String(value) : value.toFixed(2);
}

export function describeVideo(info: MediaInfo): string {
  const range = info.hdr === "none" ? "SDR" : info.hdr === "pq" ? "HDR10" : "HDR (HLG)";
  return `${info.width}×${info.height} · ${fps(info.fps)} fps · ${range}`;
}

export function fileName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

export function ago(unixSecs: number): string {
  const diff = Date.now() / 1000 - unixSecs;
  if (diff < 90) return "just now";
  if (diff < 3600) return `${Math.round(diff / 60)} min ago`;
  if (diff < 86400) return `${Math.round(diff / 3600)} h ago`;
  return new Date(unixSecs * 1000).toLocaleDateString(undefined, { day: "numeric", month: "short" });
}

// Rough running cost, in US dollars: transcription is billed per hour of
// audio; Claude translation is an estimate from typical speech density, and
// Gemini's free tier costs nothing.
const TRANSCRIBE_USD_PER_MIN = 0.22 / 60;
const CLAUDE_USD_PER_MIN = 0.04;

export function estimateCostUsd(
  durationSecs: number,
  steps: { transcribe: boolean; translate: "gemini" | "claude" | null },
): number {
  const minutes = durationSecs / 60;
  const transcribe = steps.transcribe ? TRANSCRIBE_USD_PER_MIN * minutes : 0;
  const translate = steps.translate === "claude" ? CLAUDE_USD_PER_MIN * minutes : 0;
  return transcribe + translate;
}

export function usd(amount: number): string {
  if (amount === 0) return "$0";
  if (amount < 0.01) return "less than $0.01";
  return amount < 1 ? `$${amount.toFixed(2)}` : `$${amount.toFixed(1)}`;
}
