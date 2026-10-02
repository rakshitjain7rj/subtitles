//! Projects on disk: one folder per video holding `project.json`, the
//! preview proxy and the extracted audio, so nothing paid for is redone.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::ass::Style;
use crate::captions::Caption;
use crate::error::{msg, Result};
use crate::probe::MediaInfo;
use crate::quality::QualityReport;
use crate::transcribe::Word;

const PROJECT_FILE: &str = "project.json";
pub const PREVIEW_FILE: &str = "preview.mp4";
/// Holds the source's path when the source itself is played as the preview.
pub const PREVIEW_SOURCE_FILE: &str = "preview.source";
pub const AUDIO_FILE: &str = "audio.flac";
/// A small frame from the video for the project list.
pub const THUMB_FILE: &str = "thumb.jpg";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportRecord {
    pub path: String,
    pub at: u64,
    pub report: QualityReport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub source_path: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub info: MediaInfo,
    /// The transcript, once paid for. `None` until transcription has run.
    pub words: Option<Vec<Word>>,
    pub language: Option<String>,
    pub translated: bool,
    /// The model that wrote the current captions, e.g. "Gemini 3.8 Flash".
    #[serde(default)]
    pub translated_with: Option<String>,
    pub captions: Vec<Caption>,
    pub style: Style,
    pub last_export: Option<ExportRecord>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    pub source_path: String,
    pub updated_at: u64,
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub stage: &'static str,
    pub source_missing: bool,
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Project {
    pub fn new(source: &Path, info: MediaInfo) -> Project {
        let name = source
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled")
            .to_string();
        let at = now();
        Project {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            source_path: source.to_string_lossy().into_owned(),
            created_at: at,
            updated_at: at,
            info,
            words: None,
            language: None,
            translated: false,
            translated_with: None,
            captions: Vec::new(),
            style: Style::default(),
            last_export: None,
        }
    }

    fn stage(&self) -> &'static str {
        if self.last_export.is_some() {
            "exported"
        } else if self.translated {
            "captioned"
        } else if self.words.is_some() {
            "transcribed"
        } else {
            "new"
        }
    }

    fn summary(&self) -> ProjectSummary {
        ProjectSummary {
            id: self.id.clone(),
            name: self.name.clone(),
            source_path: self.source_path.clone(),
            updated_at: self.updated_at,
            duration: self.info.duration,
            width: self.info.width,
            height: self.info.height,
            stage: self.stage(),
            source_missing: !Path::new(&self.source_path).is_file(),
        }
    }
}

/// Project ids name folders, so only the ids we generate are accepted.
pub fn valid_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok()
}

pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: PathBuf) -> Store {
        Store { root }
    }

    pub fn dir(&self, id: &str) -> Result<PathBuf> {
        if !valid_id(id) {
            return Err(msg("Unknown project."));
        }
        Ok(self.root.join(id))
    }

    pub fn load(&self, id: &str) -> Result<Project> {
        let path = self.dir(id)?.join(PROJECT_FILE);
        let text = std::fs::read_to_string(&path).map_err(|_| msg("This project no longer exists."))?;
        Ok(serde_json::from_str(&text)?)
    }

    /// Writes via a temporary file so a crash can't leave half a project.
    pub fn save(&self, project: &mut Project) -> Result<()> {
        project.updated_at = now();
        let dir = self.dir(&project.id)?;
        std::fs::create_dir_all(&dir)?;
        let tmp = dir.join(format!("{PROJECT_FILE}.tmp"));
        std::fs::write(&tmp, serde_json::to_vec_pretty(project)?)?;
        std::fs::rename(tmp, dir.join(PROJECT_FILE))?;
        Ok(())
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let dir = self.dir(id)?;
        if dir.is_dir() {
            std::fs::remove_dir_all(dir)?;
        }
        Ok(())
    }

    /// Newest first. Folders that don't hold a readable project are skipped.
    pub fn list(&self) -> Vec<ProjectSummary> {
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return Vec::new();
        };
        let mut all: Vec<ProjectSummary> = entries
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter_map(|id| self.load(&id).ok())
            .map(|p| p.summary())
            .collect();
        all.sort_by_key(|p| std::cmp::Reverse(p.updated_at));
        all
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::parse_probe;

    fn info() -> MediaInfo {
        let json = r#"{"streams":[{"codec_type":"video","codec_name":"h264","width":1080,"height":1920,
            "pix_fmt":"yuv420p","avg_frame_rate":"30/1"}],"format":{"duration":"3.0"}}"#;
        parse_probe(json, 1).unwrap()
    }

    #[test]
    fn save_load_list_delete() {
        let root = std::env::temp_dir().join(format!("subtitles-test-{}", uuid::Uuid::new_v4()));
        let store = Store::new(root.clone());
        let mut p = Project::new(Path::new("/videos/My Reel.mp4"), info());
        assert_eq!(p.name, "My Reel");
        store.save(&mut p).unwrap();

        let loaded = store.load(&p.id).unwrap();
        assert_eq!(loaded.source_path, "/videos/My Reel.mp4");
        assert!(loaded.words.is_none());

        let list = store.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].stage, "new");
        assert!(list[0].source_missing);

        store.delete(&p.id).unwrap();
        assert!(store.load(&p.id).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ids_cannot_escape_the_projects_folder() {
        let store = Store::new(PathBuf::from("/tmp/projects"));
        assert!(store.dir("../../etc").is_err());
        assert!(store.dir("").is_err());
    }
}
