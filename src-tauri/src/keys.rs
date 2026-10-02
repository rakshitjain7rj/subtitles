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

async fn set(provider: Provider, key: String) -> Result<()> {
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

/// A cheap read-only request that every valid key may make, so a mistyped
/// key is caught when it is pasted rather than halfway through a video.
fn check_request(provider: Provider, key: &str) -> reqwest::RequestBuilder {
    let client = reqwest::Client::new();
    match provider {
        Provider::ElevenLabs => client.get("https://api.elevenlabs.io/v1/user").header("xi-api-key", key),
        Provider::Gemini => client
            .get("https://generativelanguage.googleapis.com/v1beta/models?pageSize=1")
            .header("x-goog-api-key", key),
        Provider::Anthropic => client
            .get("https://api.anthropic.com/v1/models?limit=1")
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01"),
    }
}

/// Whether a check's answer means the service doesn't know the key. Anything
/// else (success, a key restricted to fewer permissions, an outage) is not
/// treated as a bad key.
fn rejects_key(provider: Provider, status: u16, body: &str) -> bool {
    match provider {
        Provider::ElevenLabs => body.contains("invalid_api_key"),
        Provider::Gemini => body.contains("API_KEY_INVALID") || status == 401 || status == 403,
        Provider::Anthropic => status == 401,
    }
}

/// Checks `key` with its service, then saves it. A key the service rejects
/// is not saved. If the service can't be reached the key is saved anyway and
/// the returned note says it wasn't checked.
pub async fn check_and_set(provider: Provider, key: String) -> Result<Option<String>> {
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err(msg("The API key is empty."));
    }
    let request = check_request(provider, &key).timeout(std::time::Duration::from_secs(15));
    let note = match request.send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            if rejects_key(provider, status, &body) {
                return Err(msg(format!(
                    "{} doesn't recognise this key. Copy it again, without spaces, and paste it here.",
                    provider.label()
                )));
            }
            None
        }
        Err(_) => Some(format!(
            "Couldn't reach {} to check the key, so it was saved unchecked.",
            provider.label()
        )),
    };
    set(provider, key).await?;
    Ok(note)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bad_keys_are_recognised_from_each_services_answer() {
        let eleven = r#"{"detail":{"status":"invalid_api_key","message":"Invalid API key"}}"#;
        assert!(rejects_key(Provider::ElevenLabs, 401, eleven));
        let scoped = r#"{"detail":{"status":"missing_permissions"}}"#;
        assert!(!rejects_key(Provider::ElevenLabs, 401, scoped));

        let gemini = r#"{"error":{"code":400,"details":[{"reason":"API_KEY_INVALID"}]}}"#;
        assert!(rejects_key(Provider::Gemini, 400, gemini));
        assert!(!rejects_key(Provider::Gemini, 503, "busy"));

        assert!(rejects_key(Provider::Anthropic, 401, ""));
        assert!(!rejects_key(Provider::Anthropic, 200, "{}"));
    }
}
