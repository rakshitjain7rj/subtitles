//! Anthropic Claude, through the Messages API.

use std::time::Duration;

use serde_json::{json, Value};

use super::{error_message, parse_captions_json, Reply, Translator, SYSTEM_PROMPT};
use crate::captions::Phrase;
use crate::error::{msg, Result};

const ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
const MODEL: &str = "claude-sonnet-5-5";
const MODEL_LABEL: &str = "Claude Sonnet 5.5";
const MAX_TOKENS: u32 = 16000;

pub struct Claude {
    api_key: String,
}

impl Claude {
    pub fn new(api_key: String) -> Self {
        Claude { api_key }
    }
}

impl Translator for Claude {
    async fn request(&self, user: &str) -> Result<Reply> {
        let client = reqwest::Client::builder().timeout(Duration::from_secs(600)).build()?;
        let resp = client
            .post(ENDPOINT)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("anthropic-beta", "server-side-fallback-2026-07-01")
            .json(&request_body(user))
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            let hint = match status.as_u16() {
                401 => " Check the Anthropic API key in Settings.",
                400 if text.contains("credit") => " The Anthropic account may be out of credit.",
                429 | 529 => " The service is busy or rate limited; try again in a minute.",
                _ => "",
            };
            return Err(msg(format!(
                "Translation failed ({status}).{hint}\n{}",
                error_message(&text)
            )));
        }
        Ok(Reply {
            phrases: parse_response(&text)?,
            model: MODEL_LABEL,
        })
    }
}

fn request_body(user: &str) -> Value {
    json!({
        "model": MODEL,
        "max_tokens": MAX_TOKENS,
        "system": SYSTEM_PROMPT,
        "messages": [{"role": "user", "content": user}],
        // If a safety classifier declines the request, retry it server-side
        // on the model Anthropic recommends instead of failing the clip.
        "fallbacks": "default",
        "output_config": {
            "effort": "medium",
            "format": {
                "type": "json_schema",
                "schema": {
                    "type": "object",
                    "properties": {
                        "captions": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "first": {"type": "integer"},
                                    "last": {"type": "integer"},
                                    "english": {"type": "string"}
                                },
                                "required": ["first", "last", "english"],
                                "additionalProperties": false
                            }
                        }
                    },
                    "required": ["captions"],
                    "additionalProperties": false
                }
            }
        }
    })
}

/// Extracts the phrases from a Messages API response body.
pub fn parse_response(response: &str) -> Result<Vec<Phrase>> {
    let root: Value = serde_json::from_str(response)?;
    match root.get("stop_reason").and_then(Value::as_str) {
        Some("refusal") => {
            return Err(msg(
                "Claude declined to translate this clip, so no captions were written.",
            ));
        }
        Some("max_tokens") => {
            return Err(msg("The translation was cut off before it finished. Try again."));
        }
        _ => {}
    }
    // Thinking and fallback blocks can come before the text block.
    let text = root
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|b| b.get("text").and_then(Value::as_str))
        .next_back()
        .ok_or_else(|| msg("The translation came back empty."))?;
    parse_captions_json(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_text_block_after_thinking() {
        let response = r#"{"stop_reason":"end_turn","content":[
            {"type":"thinking","thinking":"","signature":"x"},
            {"type":"text","text":"{\"captions\":[{\"first\":0,\"last\":1,\"english\":\"Hello friends\"}]}"}]}"#;
        let phrases = parse_response(response).unwrap();
        assert_eq!(
            phrases,
            vec![Phrase {
                first: 0,
                last: 1,
                english: "Hello friends".into()
            }]
        );
    }

    #[test]
    fn refusal_is_an_error() {
        let response = r#"{"stop_reason":"refusal","stop_details":{"type":"refusal","category":null},"content":[]}"#;
        assert!(parse_response(response).is_err());
    }

    #[test]
    fn request_names_the_model() {
        let body = request_body("hi");
        assert_eq!(body["model"], "claude-sonnet-5-5");
        assert_eq!(body["messages"][0]["content"], "hi");
        assert_eq!(body["output_config"]["format"]["type"], "json_schema");
    }
}
