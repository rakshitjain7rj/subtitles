//! App preferences kept in `settings.json` next to the projects folder.
//! API keys are not stored here; they live in the keychain (`keys.rs`).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::encode::Quality;
use crate::error::Result;
use crate::translate::Engine;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Which service translates transcripts into captions.
    pub translator: Engine,
    /// High (default) or lossless export.
    pub export_quality: Quality,
}

pub struct SettingsFile {
    path: PathBuf,
}

impl SettingsFile {
    pub fn new(path: PathBuf) -> Self {
        SettingsFile { path }
    }

    /// Missing or unreadable settings fall back to the defaults.
    pub fn load(&self) -> Settings {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, settings: &Settings) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(settings)?)?;
        std::fs::rename(tmp, &self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_defaults_when_missing_or_broken() {
        let dir = std::env::temp_dir().join(format!("subtitles-settings-{}", uuid::Uuid::new_v4()));
        let file = SettingsFile::new(dir.join("settings.json"));
        assert_eq!(file.load().translator, Engine::Gemini);

        file.save(&Settings {
            translator: Engine::Claude,
            export_quality: Quality::Lossless,
        })
        .unwrap();
        assert_eq!(file.load().translator, Engine::Claude);
        assert_eq!(file.load().export_quality, Quality::Lossless);

        // Settings written before a field existed keep the new field's default.
        std::fs::write(dir.join("settings.json"), r#"{"translator":"claude"}"#).unwrap();
        assert_eq!(file.load().export_quality, Quality::High);

        std::fs::write(dir.join("settings.json"), "not json").unwrap();
        assert_eq!(file.load(), Settings::default());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
