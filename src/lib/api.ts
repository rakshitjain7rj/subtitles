import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Caption, Engine, ProgressEvent, Quality, ProjectSummary, ProjectView, Provider, Status, Style } from "./types";

export const api = {
  status: () => invoke<Status>("app_status"),
  setExportQuality: (quality: Quality) => invoke<void>("set_export_quality", { quality }),
  setTranslator: (engine: Engine) => invoke<void>("set_translator", { engine }),
  setApiKey: (provider: Provider, key: string) => invoke<void>("set_api_key", { provider, key }),
  deleteApiKey: (provider: Provider) => invoke<void>("delete_api_key", { provider }),
  listProjects: () => invoke<ProjectSummary[]>("list_projects"),
  importVideo: (path: string) => invoke<ProjectView>("import_video", { path }),
  openProject: (id: string) => invoke<ProjectView>("open_project", { id }),
  deleteProject: (id: string) => invoke<void>("delete_project", { id }),
  saveEdits: (id: string, captions: Caption[], style: Style) => invoke<void>("save_edits", { id, captions, style }),
  transcribe: (id: string, force: boolean) => invoke<ProjectView>("transcribe_project", { id, force }),
  translate: (id: string) => invoke<ProjectView>("translate_project", { id }),
  defaultExportPath: (id: string) => invoke<string>("default_export_path", { id }),
  /** `wrapped` is each caption's text with its line breaks, from `wrapAll`. */
  exportProject: (id: string, outPath: string, wrapped: string[]) =>
    invoke<ProjectView>("export_project", { id, outPath, wrapped }),
};

export function onProgress(handler: (event: ProgressEvent) => void): Promise<UnlistenFn> {
  return listen<ProgressEvent>("progress", (e) => handler(e.payload));
}

/** Commands reject with a plain string; anything else is stringified. */
export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}
