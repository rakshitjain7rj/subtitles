//! Google Gemini, through `generateContent`. All the models used here are on
//! Google's free tier, where Google may use requests to improve its products.
//! Free-tier models are often briefly overloaded, so busy replies are retried
//! and then handed to the next model.

use std::time::Duration;

use serde_json::{json, Value};

use super::{error_message, parse_captions_json, Reply, Translator, SYSTEM_PROMPT};
use crate::captions::Phrase;
use crate::error::{msg, Result};

/// Tried in order: (model code, name shown to the user). All are on the free
/// tier; the older ones are usually less crowded when the newest is busy.
const MODELS: [(&str, &str); 4] = [
    ("gemini-3.8-flash", "Gemini 3.8 Flash"),
    ("gemini-3.7-flash", "Gemini 3.7 Flash"),
    ("gemini-3.5-flash", "Gemini 3.5 Flash"),
    ("gemini-2.5-flash", "Gemini 2.5 Flash"),
];
/// Waits before each retry of a busy model, before moving to the next one.
const RETRY_DELAYS: [Duration; 1] = [Duration::from_secs(4)];
/// Gemini counts its thinking against this limit too, so it is set well
/// above the size of the captions themselves.
const MAX_OUTPUT_TOKENS: u32 = 32768;

pub struct Gemini {
    api_key: String,
}

impl Gemini {
    pub fn new(api_key: String) -> Self {
        Gemini { api_key }
    }
}

fn endpoint(model: &str) -> String {
    format!("https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent")
}

/// What to do after an unsuccessful HTTP status.
#[derive(Debug, PartialEq)]
enum OnError {
    /// Overloaded or a server hiccup: wait and ask the same model again.
    Retry,
    /// This model's free-tier quota is used up (another model has its own),
    /// or the model has been retired.
    NextModel,
    /// Retrying won't help (bad key, bad request).
    Stop,
}

fn on_error(status: u16) -> OnError {
    match status {
        500 | 502 | 503 | 504 => OnError::Retry,
        429 | 404 => OnError::NextModel,
        _ => OnError::Stop,
    }
}

fn failure(status: reqwest::StatusCode, body: &str) -> crate::error::AppError {
    let hint = match status.as_u16() {
        400 if body.contains("API_KEY_INVALID") || body.contains("API key not valid") => {
            " Check the Gemini API key in Settings."
        }
        401 | 403 => " Check the Gemini API key in Settings.",
        429 => " The free tier's request limit was reached; wait a minute and try again.",
        500..=504 => " Gemini's free models are busy right now; wait a few minutes and use Translate.",
        _ => "",
    };
    msg(format!("Translation failed ({status}).{hint}\n{}", error_message(body)))
}

impl Translator for Gemini {
    async fn request(&self, user: &str) -> Result<Reply> {
        let client = reqwest::Client::builder().timeout(Duration::from_secs(600)).build()?;
        let body = request_body(user);
        let mut last_error = None;
        'models: for (model, label) in MODELS {
            for attempt in 0..=RETRY_DELAYS.len() {
                if attempt > 0 {
                    tokio::time::sleep(RETRY_DELAYS[attempt - 1]).await;
                }
                let resp = client
                    .post(endpoint(model))
                    // In a header rather than `?key=`, so it never lands in a URL or log.
                    .header("x-goog-api-key", &self.api_key)
                    .json(&body)
                    .send()
                    .await?;
                let status = resp.status();
                let text = resp.text().await?;
                if status.is_success() {
                    return Ok(Reply {
                        phrases: parse_response(&text)?,
                        model: label,
                    });
                }
                let action = on_error(status.as_u16());
                // A retired fallback model shouldn't hide why the others failed.
                if status.as_u16() != 404 || last_error.is_none() {
                    last_error = Some(failure(status, &text));
                }
                match action {
                    OnError::Retry => continue,
                    OnError::NextModel => continue 'models,
                    OnError::Stop => break 'models,
                }
            }
        }
        Err(last_error.unwrap_or_else(|| msg("Translation failed.")))
    }
}

fn request_body(user: &str) -> Value {
    json!({
        "systemInstruction": {"parts": [{"text": SYSTEM_PROMPT}]},
        "contents": [{"role": "user", "parts": [{"text": user}]}],
        "generationConfig": {
            "maxOutputTokens": MAX_OUTPUT_TOKENS,
            "responseMimeType": "application/json",
            "responseSchema": {
                "type": "OBJECT",
                "properties": {
                    "captions": {
                        "type": "ARRAY",
                        "items": {
                            "type": "OBJECT",
                            "properties": {
                                "first": {"type": "INTEGER"},
                                "last": {"type": "INTEGER"},
                                "english": {"type": "STRING"}
                            },
                            "required": ["first", "last", "english"],
                            "propertyOrdering": ["first", "last", "english"]
                        }
                    }
                },
                "required": ["captions"]
            }
        }
    })
}

/// Extracts the phrases from a `generateContent` response body.
pub fn parse_response(response: &str) -> Result<Vec<Phrase>> {
    let root: Value = serde_json::from_str(response)?;
    if let Some(reason) = root
        .get("promptFeedback")
        .and_then(|f| f.get("blockReason"))
        .and_then(Value::as_str)
    {
        return Err(msg(format!(
            "Gemini refused to translate this clip ({reason}), so no captions were written."
        )));
    }
    let candidate = root
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|c| c.first())
        .ok_or_else(|| msg("The translation came back empty."))?;
    match candidate.get("finishReason").and_then(Value::as_str) {
        None | Some("STOP") => {}
        Some("MAX_TOKENS") => {
            return Err(msg("The translation was cut off before it finished. Try again."));
        }
        Some(other) => {
            return Err(msg(format!(
                "Gemini stopped before finishing the translation ({other}), so no captions were written."
            )));
        }
    }
    // Skip thought summaries; the answer is the remaining text.
    let text: String = candidate
        .get("content")
        .and_then(|c| c.get("parts"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|p| p.get("thought").and_then(Value::as_bool) != Some(true))
        .filter_map(|p| p.get("text").and_then(Value::as_str))
        .collect();
    if text.trim().is_empty() {
        return Err(msg("The translation came back empty."));
    }
    parse_captions_json(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_answer_and_skips_thoughts() {
        let response = r#"{"candidates":[{"finishReason":"STOP","content":{"role":"model","parts":[
            {"text":"thinking it over","thought":true},
            {"text":"{\"captions\":[{\"first\":0,\"last\":2,\"english\":\"Hello friends\"}]}"}]}}],
            "usageMetadata":{"promptTokenCount":10}}"#;
        assert_eq!(
            parse_response(response).unwrap(),
            vec![Phrase {
                first: 0,
                last: 2,
                english: "Hello friends".into()
            }]
        );
    }

    #[test]
    fn blocked_and_truncated_replies_are_errors() {
        let blocked = r#"{"promptFeedback":{"blockReason":"SAFETY"}}"#;
        assert!(parse_response(blocked).unwrap_err().to_string().contains("SAFETY"));
        let cut = r#"{"candidates":[{"finishReason":"MAX_TOKENS","content":{"parts":[{"text":"{\"capt"}]}}]}"#;
        assert!(parse_response(cut).unwrap_err().to_string().contains("cut off"));
        let empty = r#"{"candidates":[{"finishReason":"STOP","content":{"parts":[]}}]}"#;
        assert!(parse_response(empty).is_err());
    }

    #[test]
    fn request_asks_for_json_with_the_shared_prompt() {
        let body = request_body("hi");
        assert_eq!(body["systemInstruction"]["parts"][0]["text"], SYSTEM_PROMPT);
        assert_eq!(body["contents"][0]["parts"][0]["text"], "hi");
        assert_eq!(body["generationConfig"]["responseMimeType"], "application/json");
        assert!(endpoint(MODELS[0].0).ends_with("/models/gemini-3.8-flash:generateContent"));
    }

    #[test]
    fn busy_models_are_retried_and_exhausted_quotas_skipped() {
        assert_eq!(on_error(503), OnError::Retry);
        assert_eq!(on_error(500), OnError::Retry);
        assert_eq!(on_error(429), OnError::NextModel);
        assert_eq!(on_error(404), OnError::NextModel);
        assert_eq!(on_error(400), OnError::Stop);
        assert_eq!(on_error(403), OnError::Stop);
        let busy = r#"{"error":{"code":503,"message":"This model is currently experiencing high demand.","status":"UNAVAILABLE"}}"#;
        let text = failure(reqwest::StatusCode::SERVICE_UNAVAILABLE, busy).to_string();
        assert!(text.contains("busy right now") && text.ends_with("experiencing high demand."));
        assert!(!text.contains('{'));
    }
}
