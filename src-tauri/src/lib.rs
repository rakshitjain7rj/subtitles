mod ass;
mod captions;
mod encode;
mod error;
mod ffmpeg;
mod keys;
mod pipeline;
mod preview_server;
mod probe;
mod project;
mod quality;
mod settings;
mod transcribe;
mod translate;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::ass::Style;
use crate::captions::Caption;
use crate::encode::Quality;
use crate::error::{msg, Result};
use crate::ffmpeg::Tools;
use crate::keys::Provider;
use crate::pipeline::{Stage, ToolCheck};
use crate::preview_server::PreviewServer;
use crate::project::{Project, ProjectSummary, Store};
use crate::settings::{Settings, SettingsFile};
use crate::translate::Engine;

struct AppState {
    tools: Tools,
    store: Store,
    settings: SettingsFile,
    preview: PreviewServer,
    /// Projects with a long-running step in flight.
    busy: Mutex<HashSet<String>>,
}

/// Holds a project's "busy" flag for the length of one step, so a double
/// click can't start (and pay for) the same work twice.
struct BusyGuard<'a> {
    state: &'a AppState,
    key: String,
}

impl AppState {
    fn begin(&self, key: &str) -> Result<BusyGuard<'_>> {
        let mut busy = self.busy.lock().unwrap_or_else(|e| e.into_inner());
        if !busy.insert(key.to_string()) {
            return Err(msg("This video is already being worked on."));
        }
        Ok(BusyGuard {
            state: self,
            key: key.to_string(),
        })
    }
}

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.state
            .busy
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.key);
    }
}

#[derive(Clone, Serialize)]
struct ProgressEvent {
    project_id: Option<String>,
    stage: Stage,
    fraction: Option<f64>,
}

fn progress_emitter(app: AppHandle, project_id: Option<String>) -> impl Fn(Stage, Option<f64>) + Send + Sync {
    move |stage, fraction| {
        let _ = app.emit(
            "progress",
            ProgressEvent {
                project_id: project_id.clone(),
                stage,
                fraction,
            },
        );
    }
}

#[derive(Serialize)]
struct ProjectView {
    project: Project,
    preview_url: String,
}

fn view(state: &AppState, project: Project) -> ProjectView {
    let preview_url = state.preview.url(&project.id, project.created_at);
    ProjectView { project, preview_url }
}

#[derive(Serialize)]
struct Status {
    tools: ToolCheck,
    settings: Settings,
    has_elevenlabs_key: bool,
    has_anthropic_key: bool,
    has_gemini_key: bool,
    keychain_error: Option<String>,
}

#[tauri::command]
async fn app_status(state: State<'_, AppState>) -> Result<Status> {
    let tools = pipeline::check_tools(&state.tools).await;
    let eleven = keys::get(Provider::ElevenLabs).await;
    let anthropic = keys::get(Provider::Anthropic).await;
    let gemini = keys::get(Provider::Gemini).await;
    let keychain_error = [&eleven, &anthropic, &gemini]
        .into_iter()
        .find_map(|r| r.as_ref().err())
        .map(|e| e.to_string());
    Ok(Status {
        tools,
        settings: state.settings.load(),
        has_elevenlabs_key: matches!(eleven, Ok(Some(_))),
        has_anthropic_key: matches!(anthropic, Ok(Some(_))),
        has_gemini_key: matches!(gemini, Ok(Some(_))),
        keychain_error,
    })
}

#[tauri::command]
async fn set_export_quality(state: State<'_, AppState>, quality: Quality) -> Result<()> {
    let mut settings = state.settings.load();
    settings.export_quality = quality;
    state.settings.save(&settings)
}

#[tauri::command]
async fn set_translator(state: State<'_, AppState>, engine: Engine) -> Result<()> {
    let mut settings = state.settings.load();
    settings.translator = engine;
    state.settings.save(&settings)
}

#[tauri::command]
async fn set_api_key(provider: Provider, key: String) -> Result<Option<String>> {
    keys::check_and_set(provider, key).await
}

#[tauri::command]
async fn delete_api_key(provider: Provider) -> Result<()> {
    keys::delete(provider).await
}

#[tauri::command]
async fn list_projects(state: State<'_, AppState>) -> Result<Vec<ProjectSummary>> {
    Ok(state.store.list())
}

#[tauri::command]
async fn import_video(app: AppHandle, state: State<'_, AppState>, path: String) -> Result<ProjectView> {
    let _guard = state.begin(&format!("import:{path}"))?;
    let progress = progress_emitter(app, None);
    let project = pipeline::import(&state.tools, &state.store, &PathBuf::from(path), &progress).await?;
    Ok(view(&state, project))
}

#[tauri::command]
async fn open_project(state: State<'_, AppState>, id: String) -> Result<ProjectView> {
    Ok(view(&state, state.store.load(&id)?))
}

#[tauri::command]
async fn delete_project(state: State<'_, AppState>, id: String) -> Result<()> {
    let _guard = state.begin(&id)?;
    state.store.delete(&id)
}

/// Saves caption and style edits from the review screen.
#[tauri::command]
async fn save_edits(state: State<'_, AppState>, id: String, captions: Vec<Caption>, style: Style) -> Result<()> {
    let mut project = state.store.load(&id)?;
    project.captions = captions;
    project.style = style.clamped();
    state.store.save(&mut project)
}

#[tauri::command]
async fn transcribe_project(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    force: bool,
) -> Result<ProjectView> {
    let _guard = state.begin(&id)?;
    let progress = progress_emitter(app, Some(id.clone()));
    let project = pipeline::transcribe(&state.tools, &state.store, &id, force, &progress).await?;
    Ok(view(&state, project))
}

#[tauri::command]
async fn translate_project(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<ProjectView> {
    let _guard = state.begin(&id)?;
    let progress = progress_emitter(app, Some(id.clone()));
    let engine = state.settings.load().translator;
    let project = pipeline::translate(&state.store, &id, engine, &progress).await?;
    Ok(view(&state, project))
}

#[tauri::command]
async fn default_export_path(state: State<'_, AppState>, id: String) -> Result<String> {
    let project = state.store.load(&id)?;
    Ok(pipeline::default_export_path(&project).to_string_lossy().into_owned())
}

#[tauri::command]
async fn export_project(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    out_path: String,
    wrapped: Vec<String>,
) -> Result<ProjectView> {
    let _guard = state.begin(&id)?;
    let progress = progress_emitter(app, Some(id.clone()));
    let quality = state.settings.load().export_quality;
    let project = pipeline::export(
        &state.tools,
        &state.store,
        &id,
        &PathBuf::from(out_path),
        &wrapped,
        quality,
        &progress,
    )
    .await?;
    Ok(view(&state, project))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data = app.path().app_data_dir()?;
            let projects = data.join("projects");
            std::fs::create_dir_all(&projects)?;
            let resources = app.path().resource_dir().ok();
            app.manage(AppState {
                tools: Tools::locate(resources.as_deref()),
                store: Store::new(projects.clone()),
                settings: SettingsFile::new(data.join("settings.json")),
                preview: PreviewServer::start(projects)?,
                busy: Mutex::new(HashSet::new()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_status,
            set_translator,
            set_export_quality,
            set_api_key,
            delete_api_key,
            list_projects,
            import_video,
            open_project,
            delete_project,
            save_edits,
            transcribe_project,
            translate_project,
            default_export_path,
            export_project,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
