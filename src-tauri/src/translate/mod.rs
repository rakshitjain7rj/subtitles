//! Turning a Hindi/Hinglish transcript into short English caption phrases.
//! The prompt, chunking and output checks are shared; each provider module
//! only knows how to send one request and read its answer.

use std::future::Future;

use serde::{Deserialize, Serialize};

use crate::captions::{repair_phrases, split_long_phrases, Phrase};
use crate::error::{msg, Result};
use crate::transcribe::Word;

pub mod claude;
pub mod gemini;

/// Long clips are captioned in pieces so one response never outgrows the
/// output limit; each piece still sees the full transcript.
const CHUNK_WORDS: usize = 450;
/// How far back from a chunk boundary to look for a pause to break at.
const BOUNDARY_SEARCH_WORDS: usize = 60;

pub const SYSTEM_PROMPT: &str = "\
You write English captions for short-form videos spoken in Hindi or Hinglish (Hindi mixed with English). \
The captions are burned into the video and shown a few words at a time, in sync with the speech, so that \
viewers who don't understand Hindi can follow along.

You receive the transcript as a numbered list of spoken words with start and end times in seconds. It comes \
from automatic speech recognition, so it can contain misheard words, English words may be written in \
Devanagari, and Hindi may be written in Latin script. Use the context of the whole clip to work out what \
the speaker meant.

How to work:

1. Understand each sentence before translating it. Translate the meaning, not word by word: natural, \
conversational English in the speaker's own voice and register, the way a bilingual creator would caption \
their own video. Keep names, brands and numbers accurate. Keep the tone as it is; don't sanitise, soften \
or embellish.

2. Break each translated sentence into caption phrases of 2 to 5 words, splitting where a reader would \
naturally pause so each phrase reads well on its own. Never put more than 5 words in one phrase: count \
them, and split a longer clause in two (\"He was basically the founder\" / \"of the Sikh Empire\"). A single \
word is fine for an interjection or for emphasis.

3. Give each phrase the run of spoken words during which it should be on screen, as `first` and `last` \
word numbers (inclusive). Hindi and English put words in a different order, so don't try to match words \
one to one. Instead, spread a sentence's phrases, in English reading order, across that sentence's words, \
roughly in proportion to phrase length. Keep every phrase inside its own sentence's words so captions \
never run ahead of the speaker or trail into the next sentence; a long gap between word times usually \
marks a sentence boundary. If a sentence has fewer spoken words than it would need phrases, use longer \
phrases rather than sharing a word between two.

Rules for the ranges: phrases are in order, ranges don't overlap, and every requested word number belongs \
to exactly one phrase. Filler and false starts that a captioner would leave out (um, a stumbled repeat) \
still need a range: give them a phrase whose `english` is an empty string, and nothing will be shown.

When asked for only part of the transcript, caption only the requested word numbers; the rest is there \
so you know the context.";

/// Which service translates. Chosen in Settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    /// Google Gemini; has a free tier.
    #[default]
    Gemini,
    /// Anthropic Claude; paid per use.
    Claude,
}

/// A provider's answer for one chunk.
#[derive(Debug)]
pub struct Reply {
    pub phrases: Vec<Phrase>,
    /// The model that answered, e.g. "Gemini 3.8 Flash". A provider may fall
    /// back to another model when its first choice is busy.
    pub model: &'static str,
}

/// One translation service.
pub trait Translator {
    /// Sends the shared system prompt plus `user`, and returns the phrases
    /// from the structured reply.
    fn request(&self, user: &str) -> impl Future<Output = Result<Reply>> + Send;
}

pub struct Translation {
    pub phrases: Vec<Phrase>,
    /// Every model that answered, in order, e.g. "Gemini 3.8 Flash".
    pub models: Vec<&'static str>,
}

impl Translation {
    /// "Gemini 3.8 Flash", or "Gemini 3.8 Flash + Gemini 3.7 Flash" when a
    /// long clip's chunks were answered by different models.
    pub fn model_label(&self) -> String {
        self.models.join(" + ")
    }
}

/// Translates the whole clip into phrases covering `0..words.len()`.
pub async fn translate(
    translator: &impl Translator,
    words: &[Word],
    mut on_progress: impl FnMut(f64),
) -> Result<Translation> {
    let mut out = Translation {
        phrases: Vec::new(),
        models: Vec::new(),
    };
    if words.is_empty() {
        return Ok(out);
    }
    let chunks = plan_chunks(words);
    for (i, (lo, hi)) in chunks.iter().copied().enumerate() {
        on_progress(i as f64 / chunks.len() as f64);
        let reply = translator.request(&user_message(words, lo, hi)).await?;
        out.phrases.extend(split_long_phrases(repair_phrases(reply.phrases, lo, hi)));
        if !out.models.contains(&reply.model) {
            out.models.push(reply.model);
        }
    }
    on_progress(1.0);
    Ok(out)
}

/// The human-readable part of a JSON error body (`{"error": {"message": ...}}`,
/// the shape both Google and Anthropic use), or the start of the raw body.
pub fn error_message(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("error")?.get("message")?.as_str().map(str::to_string))
        .unwrap_or_else(|| crate::transcribe::scribe::truncate(body, 300))
}

/// Inclusive word ranges, broken at the longest pause near each boundary.
pub fn plan_chunks(words: &[Word]) -> Vec<(usize, usize)> {
    let mut chunks = Vec::new();
    let mut lo = 0;
    while lo < words.len() {
        let mut hi = (lo + CHUNK_WORDS).min(words.len()) - 1;
        if hi + 1 < words.len() {
            let search_from = hi.saturating_sub(BOUNDARY_SEARCH_WORDS).max(lo);
            let gap_after = |i: usize| words[i + 1].start - words[i].end;
            hi = (search_from..=hi)
                .max_by(|a, b| gap_after(*a).total_cmp(&gap_after(*b)))
                .unwrap_or(hi);
        }
        chunks.push((lo, hi));
        lo = hi + 1;
    }
    chunks
}

/// The user turn: the full transcript for context, the timed word list, and
/// which word numbers to caption.
pub fn user_message(words: &[Word], lo: usize, hi: usize) -> String {
    let transcript = words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
    let listing = words
        .iter()
        .enumerate()
        .map(|(i, w)| format!("{i}\t{:.2}\t{:.2}\t{}", w.start, w.end, w.text))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Full transcript, for context:\n<transcript>\n{transcript}\n</transcript>\n\n\
         Spoken words (number, start, end, word):\n<words>\n{listing}\n</words>\n\n\
         Caption word numbers {lo} through {hi}."
    )
}

#[derive(Deserialize)]
struct Captions {
    captions: Vec<Phrase>,
}

/// Reads the `{"captions": [...]}` JSON both providers are asked to return.
pub fn parse_captions_json(text: &str) -> Result<Vec<Phrase>> {
    let parsed: Captions = serde_json::from_str(text.trim())
        .map_err(|e| msg(format!("The translation came back in an unexpected shape: {e}")))?;
    Ok(parsed.captions)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn words(n: usize, pause_after: &[usize]) -> Vec<Word> {
        let mut t = 0.0;
        (0..n)
            .map(|i| {
                let w = Word {
                    text: format!("w{i}"),
                    start: t,
                    end: t + 0.2,
                };
                t += if pause_after.contains(&i) { 1.5 } else { 0.25 };
                w
            })
            .collect()
    }

    #[test]
    fn short_clip_is_one_chunk() {
        assert_eq!(plan_chunks(&words(120, &[])), vec![(0, 119)]);
    }

    #[test]
    fn long_clip_breaks_at_a_pause_and_covers_everything() {
        let w = words(1000, &[430, 880]);
        let chunks = plan_chunks(&w);
        assert_eq!(chunks[0], (0, 430));
        assert_eq!(chunks[1], (431, 880));
        assert_eq!(chunks.last().unwrap().1, 999);
        for pair in chunks.windows(2) {
            assert_eq!(pair[0].1 + 1, pair[1].0);
        }
    }

    #[test]
    fn user_message_names_the_range() {
        let text = user_message(&words(3, &[]), 0, 2);
        assert!(text.contains("0\t0.00\t0.20\tw0"));
        assert!(text.ends_with("Caption word numbers 0 through 2."));
    }

    /// Stands in for a provider: answers each chunk with one phrase per word.
    struct Echo;

    impl Translator for Echo {
        async fn request(&self, user: &str) -> Result<Reply> {
            let tail = user.rsplit("Caption word numbers ").next().unwrap();
            let nums: Vec<usize> = tail
                .trim_end_matches('.')
                .split(" through ")
                .map(|n| n.parse().unwrap())
                .collect();
            Ok(Reply {
                phrases: (nums[0]..=nums[1])
                    .map(|i| Phrase {
                        first: i,
                        last: i,
                        english: format!("e{i}"),
                    })
                    .collect(),
                model: "Echo",
            })
        }
    }

    #[tokio::test]
    async fn chunks_are_stitched_back_together() {
        let w = words(1000, &[430, 880]);
        let mut progress = Vec::new();
        let result = translate(&Echo, &w, |f| progress.push(f)).await.unwrap();
        assert_eq!(result.phrases.len(), 1000);
        assert!(result.phrases.iter().enumerate().all(|(i, p)| p.first == i));
        assert_eq!(result.model_label(), "Echo");
        assert_eq!(progress.last(), Some(&1.0));
    }

    #[test]
    fn error_bodies_are_reduced_to_their_message() {
        let body = r#"{"error":{"code":503,"message":"This model is currently experiencing high demand.","status":"UNAVAILABLE"}}"#;
        assert_eq!(error_message(body), "This model is currently experiencing high demand.");
        assert_eq!(error_message("gateway timeout"), "gateway timeout");
    }

    #[test]
    fn engine_defaults_to_the_free_one() {
        assert_eq!(Engine::default(), Engine::Gemini);
        assert_eq!(serde_json::to_string(&Engine::Claude).unwrap(), "\"claude\"");
    }
}
