// Mirrors the structs the Rust side serialises.

export type Hdr = "none" | "pq" | "hlg";
export type Provider = "elevenlabs" | "anthropic" | "gemini";
/** Which service translates the transcript. */
export type Engine = "gemini" | "claude";
/** Export mode: visually identical at a sensible size, or exact. */
export type Quality = "high" | "lossless";
export type Stage = "preview" | "audio" | "transcribe" | "translate" | "encode" | "verify";

export interface MediaInfo {
  width: number;
  height: number;
  rotation: number;
  fps: number;
  duration: number;
  frame_count: number | null;
  video_codec: string;
  pix_fmt: string;
  bit_depth: number;
  hdr: Hdr;
  dolby_vision: boolean;
  audio_codec: string | null;
  video_bitrate: number | null;
  size_bytes: number;
}

export interface Word {
  text: string;
  start: number;
  end: number;
}

export interface Caption {
  id: string;
  start: number;
  end: number;
  english: string;
  hindi: string;
}

export interface Style {
  /** Vertical centre of the caption, percent of height from the top. */
  y_pct: number;
  /** Text size, percent of the video's shorter side. */
  size_pct: number;
}

export interface VmafStats {
  mean: number;
  min: number;
  harmonic_mean: number;
  /** Luma PSNR in dB; 60 means identical. Missing on older reports. */
  psnr_y?: number | null;
}

export interface QualityReport {
  vmaf: VmafStats | null;
  /** The original scored against itself: the best this video can get. Missing on older reports. */
  ceiling?: number | null;
  /** The file is written and its score is still being measured. */
  pending?: boolean;
  quality?: Quality;
  measured_fraction: number;
  frame_step: number;
  resolution_match: boolean;
  fps_match: boolean;
  frame_count_match: boolean | null;
  audio_match: boolean;
  hdr_match: boolean;
  source: MediaInfo;
  export: MediaInfo;
  notes: string[];
}

export interface ExportRecord {
  path: string;
  at: number;
  report: QualityReport;
}

export interface Project {
  id: string;
  name: string;
  source_path: string;
  created_at: number;
  updated_at: number;
  info: MediaInfo;
  words: Word[] | null;
  language: string | null;
  translated: boolean;
  /** The model that wrote the current captions, e.g. "Gemini 3.8 Flash". */
  translated_with: string | null;
  captions: Caption[];
  style: Style;
  last_export: ExportRecord | null;
}

export interface ProjectView {
  project: Project;
  preview_url: string;
  /** False until `buildPreview` has made the preview. */
  preview_ready: boolean;
}

export interface ProjectSummary {
  id: string;
  name: string;
  source_path: string;
  updated_at: number;
  duration: number;
  width: number;
  height: number;
  stage: "new" | "transcribed" | "captioned" | "exported";
  source_missing: boolean;
  /** A frame from the video, once made. */
  thumb_url: string | null;
}

export interface ToolCheck {
  ok: boolean;
  version: string | null;
  missing: string[];
}

export interface Status {
  tools: ToolCheck;
  settings: { translator: Engine; export_quality: Quality };
  has_elevenlabs_key: boolean;
  has_anthropic_key: boolean;
  has_gemini_key: boolean;
  keychain_error: string | null;
}

/** True when the key the chosen translator needs is saved. */
export function hasTranslatorKey(status: Status): boolean {
  return status.settings.translator === "gemini" ? status.has_gemini_key : status.has_anthropic_key;
}

export interface ProgressEvent {
  project_id: string | null;
  stage: Stage;
  fraction: number | null;
}
