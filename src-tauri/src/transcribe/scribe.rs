//! ElevenLabs Scribe v2.

use std::path::Path;
use std::time::Duration;

use reqwest::multipart::{Form, Part};
use serde::Deserialize;

use super::{Transcriber, Transcript, Word};
use crate::error::{msg, Result};

const ENDPOINT: &str = "https://api.elevenlabs.io/v1/speech-to-text";
const MODEL: &str = "scribe_v2";
/// Hinglish is transcribed as Hindi; leaving detection on risks a clip with a
/// lot of English being treated as English and the Hindi garbled.
const LANGUAGE: &str = "hi";

pub struct Scribe {
    api_key: String,
}

impl Scribe {
    pub fn new(api_key: String) -> Self {
        Scribe { api_key }
    }
}

#[derive(Deserialize)]
struct ScribeResponse {
    language_code: Option<String>,
    #[serde(default)]
    words: Vec<ScribeWord>,
}

#[derive(Deserialize)]
struct ScribeWord {
    text: String,
    #[serde(rename = "type", default)]
    kind: Option<String>,
    start: Option<f64>,
    end: Option<f64>,
}

/// Keeps spoken words only; spacing and audio-event entries are dropped.
pub fn parse_response(json: &str) -> Result<Transcript> {
    let resp: ScribeResponse = serde_json::from_str(json)?;
    let words = resp
        .words
        .into_iter()
        .filter(|w| w.kind.as_deref().unwrap_or("word") == "word" && !w.text.trim().is_empty())
        .filter_map(|w| {
            Some(Word {
                text: w.text.trim().to_string(),
                start: w.start?,
                end: w.end?,
            })
        })
        .collect();
    Ok(Transcript {
        language: resp.language_code,
        words,
    })
}

impl Transcriber for Scribe {
    async fn transcribe(&self, audio: &Path) -> Result<Transcript> {
        let bytes = tokio::fs::read(audio).await?;
        let name = audio
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("audio.flac")
            .to_string();
        let form = Form::new()
            .text("model_id", MODEL)
            .text("language_code", LANGUAGE)
            .text("timestamps_granularity", "word")
            .text("tag_audio_events", "false")
            .text("diarize", "false")
            .part("file", Part::bytes(bytes).file_name(name));

        let client = reqwest::Client::builder().timeout(Duration::from_secs(600)).build()?;
        let resp = client
            .post(ENDPOINT)
            .header("xi-api-key", &self.api_key)
            .multipart(form)
            .send()
            .await?;
        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() {
            let hint = match status.as_u16() {
                401 => " Check the ElevenLabs API key in Settings.",
                402 | 429 => " The ElevenLabs account may be out of credit or rate limited.",
                _ => "",
            };
            return Err(msg(format!(
                "Transcription failed ({status}).{hint}\n{}",
                truncate(&body, 400)
            )));
        }
        let transcript = parse_response(&body)?;
        if transcript.words.is_empty() {
            return Err(msg("No speech was found in this video."));
        }
        Ok(transcript)
    }
}

pub(crate) fn truncate(s: &str, max_chars: usize) -> String {
    match s.char_indices().nth(max_chars) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_timed_words() {
        let json = r#"{"language_code":"hi","language_probability":0.98,"text":"नमस्ते दोस्तों",
          "words":[
            {"text":"नमस्ते","type":"word","start":0.1,"end":0.5,"logprob":-0.1},
            {"text":" ","type":"spacing","start":0.5,"end":0.6},
            {"text":"(music)","type":"audio_event","start":0.6,"end":0.9},
            {"text":"दोस्तों","type":"word","start":0.9,"end":1.4}
          ]}"#;
        let t = parse_response(json).unwrap();
        assert_eq!(t.language.as_deref(), Some("hi"));
        assert_eq!(t.words.len(), 2);
        assert_eq!(
            t.words[1],
            Word {
                text: "दोस्तों".into(),
                start: 0.9,
                end: 1.4
            }
        );
    }
}
