//! The user's own API keys, kept in the operating system's keychain
//! (Secret Service on Linux, Keychain on macOS, Credential Manager on Windows).

use serde::{Deserialize, Serialize};

use crate::error::{msg, Result};

const KEYCHAIN_SERVICE: &str = "com.subtitles.desktop";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    ElevenLabs,
    Anthropic,
    Gemini,
}

impl Provider {
    fn account(self) -> &'static str {
        match self {
            Provider::ElevenLabs => "elevenlabs-api-key",
            Provider::Anthropic => "anthropic-api-key",
            Provider::Gemini => "gemini-api-key",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Provider::ElevenLabs => "ElevenLabs",
            Provider::Anthropic => "Anthropic",
            Provider::Gemini => "Gemini",
        }
    }
}

fn keychain_error(e: keyring::Error) -> crate::error::AppError {
    msg(format!("Could not reach the system keychain: {e}"))
}

// Keychain calls block (and on Linux drive their own D-Bus runtime), so they
// run off the async executor.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| msg(format!("Keychain task failed: {e}")))?
}

pub async fn get(provider: Provider) -> Result<Option<String>> {
    blocking(move || {
        let entry = keyring::Entry::new(KEYCHAIN_SERVICE, provider.account()).map_err(keychain_error)?;
        match entry.get_password() {
            Ok(key) => Ok(Some(key)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(keychain_error(e)),
        }
    })
    .await
}

/// The key for a provider, or an error telling the user where to add it.
pub async fn require(provider: Provider) -> Result<String> {
    get(provider)
        .await?
        .ok_or_else(|| msg(format!("Add your {} API key in Settings first.", provider.label())))
}

pub async fn set(provider: Provider, key: String) -> Result<()> {
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err(msg("The API key is empty."));
    }
    blocking(move || {
        let entry = keyring::Entry::new(KEYCHAIN_SERVICE, provider.account()).map_err(keychain_error)?;
        entry.set_password(&key).map_err(keychain_error)
    })
    .await
}

pub async fn delete(provider: Provider) -> Result<()> {
    blocking(move || {
        let entry = keyring::Entry::new(KEYCHAIN_SERVICE, provider.account()).map_err(keychain_error)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(keychain_error(e)),
        }
    })
    .await
}
