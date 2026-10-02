//! Google Gemini, through `generateContent`. All the models used here are on
//! Google's free tier, where Google may use requests to improve its products.
//! Free-tier models are often overloaded, and a busy reply can take minutes
//! to arrive, so a busy model hands over to the next one at once and a slow
//! one gets the next one started alongside it. Every model is asked with the
//! same full-quality request; only who answers first changes.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use tokio::task::JoinSet;

use serde_json::{json, Value};

use super::{error_message, parse_captions_json, Reply, Translator, SYSTEM_PROMPT};
use crate::captions::Phrase;
use crate::error::{msg, AppError, Result};

/// Tried in order: (model code, name shown to the user). All are on the free
/// tier; the older ones are usually less crowded when the newest is busy.
const MODELS: [(&str, &str); 3] = [
    ("gemini-3.8-flash", "Gemini 3.8 Flash"),
    ("gemini-3.7-flash", "Gemini 3.7 Flash"),
    ("gemini-3.5-flash", "Gemini 3.5 Flash"),
];
/// A model that hasn't answered by now gets the next one started alongside
/// it. A normal answer takes 20–30 s, as the model thinks first.
const HEDGE_AFTER: Duration = Duration::from_secs(45);
/// When every model was busy, the whole list is tried this many times in all,
/// this far apart.
const ROUNDS: usize = 2;
const ROUND_GAP: Duration = Duration::from_secs(4);
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
    /// Overloaded, out of free-tier quota (each model has its own) or retired:
    /// ask the next model.
    NextModel,
    /// No model will do better (bad key, bad request).
    Stop,
}

fn on_error(status: u16) -> OnError {
    match status {
        500 | 502 | 503 | 504 | 429 | 404 => OnError::NextModel,
        _ => OnError::Stop,
    }
}

/// How one model's attempt ended.
enum Attempt {
    Answered(Reply),
    /// Try another model. `quiet` errors (a retired model) don't replace a
    /// more useful earlier one in the final message.
    Next { error: AppError, quiet: bool },
    Stop(AppError),
}

/// Asks `count` models through `attempt`, in order. A model that fails hands
/// over to the next at once; one still working after `hedge_after` gets the
/// next started alongside it. The first answer wins and the rest are
/// cancelled. If every model fails, the list is tried again `rounds - 1`
/// more times, `round_gap` apart.
async fn race<F, Fut>(count: usize, hedge_after: Duration, rounds: usize, round_gap: Duration, attempt: F) -> Result<Reply>
where
    F: Fn(usize) -> Fut,
    Fut: Future<Output = Attempt> + Send + 'static,
{
    let mut last_error: Option<AppError> = None;
    for round in 0..rounds {
        if round > 0 {
            tokio::time::sleep(round_gap).await;
        }
        let mut running = JoinSet::new();
        running.spawn(attempt(0));
        let mut next = 1;
        loop {
            tokio::select! {
                joined = running.join_next() => {
                    match joined {
                        Some(Ok(Attempt::Answered(reply))) => return Ok(reply),
                        Some(Ok(Attempt::Stop(error))) => return Err(error),
                        Some(Ok(Attempt::Next { error, quiet })) => {
                            if !quiet || last_error.is_none() {
                                last_error = Some(error);
                            }
                        }
                        Some(Err(e)) => last_error = Some(msg(format!("Translation task failed: {e}"))),
                        None => {}
                    }
                    if running.is_empty() {
                        if next == count {
                            break;
                        }
                        running.spawn(attempt(next));
                        next += 1;
                    }
                }
                _ = tokio::time::sleep(hedge_after), if next < count => {
                    running.spawn(attempt(next));
                    next += 1;
                }
            }
        }
    }
    Err(last_error.unwrap_or_else(|| msg("Translation failed.")))
}

async fn ask_model(client: reqwest::Client, key: Arc<str>, body: Arc<Value>, index: usize) -> Attempt {
    let (model, label) = MODELS[index];
    let sent = client
        .post(endpoint(model))
        // In a header rather than `?key=`, so it never lands in a URL or log.
        .header("x-goog-api-key", &*key)
        .json(&*body)
        .send()
        .await;
    let resp = match sent {
        Ok(resp) => resp,
        Err(e) => return Attempt::Stop(e.into()),
    };
    let status = resp.status();
    let text = match resp.text().await {
        Ok(text) => text,
        Err(e) => return Attempt::Stop(e.into()),
    };
    if status.is_success() {
        return match parse_response(&text) {
            Ok(phrases) => Attempt::Answered(Reply { phrases, model: label }),
            Err(e) => Attempt::Stop(e),
        };
    }
    let error = failure(status, &text);
    match on_error(status.as_u16()) {
        OnError::NextModel => Attempt::Next {
            error,
            quiet: status.as_u16() == 404,
        },
        OnError::Stop => Attempt::Stop(error),
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
        let key: Arc<str> = self.api_key.as_str().into();
        let body = Arc::new(request_body(user));
        race(MODELS.len(), HEDGE_AFTER, ROUNDS, ROUND_GAP, |i| {
            ask_model(client.clone(), key.clone(), body.clone(), i)
        })
        .await
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
    fn busy_and_exhausted_models_hand_over() {
        assert_eq!(on_error(503), OnError::NextModel);
        assert_eq!(on_error(500), OnError::NextModel);
        assert_eq!(on_error(429), OnError::NextModel);
        assert_eq!(on_error(404), OnError::NextModel);
        assert_eq!(on_error(400), OnError::Stop);
        assert_eq!(on_error(403), OnError::Stop);
        let busy = r#"{"error":{"code":503,"message":"This model is currently experiencing high demand.","status":"UNAVAILABLE"}}"#;
        let text = failure(reqwest::StatusCode::SERVICE_UNAVAILABLE, busy).to_string();
        assert!(text.contains("busy right now") && text.ends_with("experiencing high demand."));
        assert!(!text.contains('{'));
    }

    /// A scripted model: answers or fails after `ms` milliseconds.
    fn scripted(script: &'static [(u64, &'static str)]) -> impl Fn(usize) -> std::pin::Pin<Box<dyn Future<Output = Attempt> + Send>> {
        move |i| {
            let (ms, outcome) = script[i];
            Box::pin(async move {
                tokio::time::sleep(Duration::from_millis(ms)).await;
                match outcome {
                    "ok" => Attempt::Answered(Reply {
                        phrases: Vec::new(),
                        model: MODELS[i].1,
                    }),
                    "busy" => Attempt::Next {
                        error: msg(format!("busy {i}")),
                        quiet: false,
                    },
                    "gone" => Attempt::Next {
                        error: msg(format!("gone {i}")),
                        quiet: true,
                    },
                    _ => Attempt::Stop(msg(format!("stop {i}"))),
                }
            })
        }
    }

    async fn run(script: &'static [(u64, &'static str)]) -> (Result<Reply>, Duration) {
        let started = std::time::Instant::now();
        let hedge = Duration::from_millis(200);
        let gap = Duration::from_millis(10);
        let result = race(script.len(), hedge, 2, gap, scripted(script)).await;
        (result, started.elapsed())
    }

    #[tokio::test]
    async fn a_busy_model_hands_over_at_once() {
        let (result, took) = run(&[(20, "busy"), (20, "ok"), (20, "ok")]).await;
        assert_eq!(result.unwrap().model, "Gemini 3.7 Flash");
        assert!(took < Duration::from_millis(150), "{took:?}");
    }

    #[tokio::test]
    async fn a_slow_model_gets_the_next_one_started_alongside() {
        let (result, took) = run(&[(2_000, "ok"), (30, "ok"), (30, "ok")]).await;
        assert_eq!(result.unwrap().model, "Gemini 3.7 Flash");
        assert!(took < Duration::from_millis(600), "{took:?}");
    }

    #[tokio::test]
    async fn a_slow_model_still_wins_if_it_answers_first() {
        let (result, _) = run(&[(300, "ok"), (2_000, "ok"), (2_000, "ok")]).await;
        assert_eq!(result.unwrap().model, "Gemini 3.8 Flash");
    }

    #[tokio::test]
    async fn all_busy_tries_the_list_again_then_reports_the_useful_error() {
        let (result, _) = run(&[(5, "busy"), (5, "busy"), (5, "gone")]).await;
        assert_eq!(result.unwrap_err().to_string(), "busy 1");
    }

    #[tokio::test]
    async fn a_bad_request_stops_everything() {
        let (result, took) = run(&[(10, "stop"), (10, "ok"), (10, "ok")]).await;
        assert_eq!(result.unwrap_err().to_string(), "stop 0");
        assert!(took < Duration::from_millis(100));
    }
}
