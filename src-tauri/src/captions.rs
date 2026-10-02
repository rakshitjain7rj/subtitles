//! Caption data and the timing rules that turn word ranges into captions.

use serde::{Deserialize, Serialize};

use crate::transcribe::Word;

/// A pause shorter than this keeps the caption up until the next one starts,
/// so the text doesn't flicker off between phrases.
const BRIDGE_GAP_SECS: f64 = 0.35;
/// How long a caption lingers into a longer pause.
const LINGER_SECS: f64 = 0.25;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Caption {
    pub id: String,
    pub start: f64,
    pub end: f64,
    pub english: String,
    pub hindi: String,
}

/// An English phrase and the inclusive range of spoken words it is shown over.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Phrase {
    pub first: usize,
    pub last: usize,
    pub english: String,
}

/// Makes model output safe to index with: in order, non-overlapping, inside
/// `lo..=hi`. Phrases that end up with no words of their own are dropped.
pub fn repair_phrases(mut phrases: Vec<Phrase>, lo: usize, hi: usize) -> Vec<Phrase> {
    phrases.sort_by_key(|p| p.first);
    let mut out: Vec<Phrase> = Vec::with_capacity(phrases.len());
    let mut next_free = lo;
    for mut p in phrases {
        p.first = p.first.max(next_free);
        p.last = p.last.min(hi);
        if p.first > p.last {
            continue;
        }
        next_free = p.last + 1;
        out.push(p);
    }
    out
}

/// Builds timed captions. Phrases with empty English (filler the translator
/// chose not to caption) produce no caption.
pub fn build_captions(words: &[Word], phrases: &[Phrase]) -> Vec<Caption> {
    let mut captions: Vec<Caption> = phrases
        .iter()
        .filter(|p| !p.english.trim().is_empty() && p.last < words.len() && p.first <= p.last)
        .map(|p| {
            let span = &words[p.first..=p.last];
            Caption {
                id: uuid::Uuid::new_v4().to_string(),
                start: span[0].start,
                end: span[span.len() - 1].end.max(span[0].start),
                english: p.english.trim().to_string(),
                hindi: span.iter().map(|w| w.text.trim()).collect::<Vec<_>>().join(" "),
            }
        })
        .collect();

    for i in 0..captions.len() {
        let next_start = captions.get(i + 1).map(|c| c.start);
        let c = &mut captions[i];
        match next_start {
            Some(next) if next - c.end < BRIDGE_GAP_SECS => c.end = next.max(c.start),
            Some(next) => c.end = (c.end + LINGER_SECS).min(next),
            None => c.end += LINGER_SECS,
        }
    }
    captions
}

fn normalised(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Applies the review screen's line breaks: `wrapped[i]` is caption `i` with
/// newlines where its lines break. Returns `None` unless every entry is the
/// caption's own text with only whitespace changed, so stale or mismatched
/// wrapping can never alter what gets burned in.
pub fn with_line_breaks(captions: &[Caption], wrapped: &[String]) -> Option<Vec<Caption>> {
    if captions.len() != wrapped.len() {
        return None;
    }
    captions
        .iter()
        .zip(wrapped)
        .map(|(c, w)| {
            (normalised(&c.english) == normalised(w)).then(|| Caption {
                english: w.trim().to_string(),
                ..c.clone()
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str, start: f64, end: f64) -> Word {
        Word {
            text: text.into(),
            start,
            end,
        }
    }

    fn phrase(first: usize, last: usize, english: &str) -> Phrase {
        Phrase {
            first,
            last,
            english: english.into(),
        }
    }

    #[test]
    fn repair_fixes_overlap_and_range() {
        let fixed = repair_phrases(
            vec![
                phrase(2, 4, "b"),
                phrase(0, 2, "a"),
                phrase(4, 4, "dropped"),
                phrase(5, 99, "c"),
            ],
            0,
            7,
        );
        assert_eq!(fixed, vec![phrase(0, 2, "a"), phrase(3, 4, "b"), phrase(5, 7, "c")]);
    }

    #[test]
    fn timing_bridges_short_gaps_and_lingers_in_long_ones() {
        let words = vec![
            word("आज", 0.0, 0.3),
            word("हम", 0.4, 0.6),
            word("बात", 0.7, 1.0),
            word("करेंगे", 3.0, 3.5),
        ];
        let caps = build_captions(
            &words,
            &[
                phrase(0, 0, "Today"),
                phrase(1, 2, "we'll talk"),
                phrase(3, 3, "about it"),
            ],
        );
        assert_eq!(caps.len(), 3);
        assert_eq!(caps[0].end, 0.4); // bridged to the next caption
        assert_eq!(caps[1].end, 1.25); // lingers into the pause
        assert_eq!(caps[1].hindi, "हम बात");
        assert_eq!(caps[2].end, 3.75);
    }

    #[test]
    fn line_breaks_apply_only_when_the_text_matches() {
        let caps = vec![
            Caption {
                id: "a".into(),
                start: 0.0,
                end: 1.0,
                english: "to save money that works".into(),
                hindi: String::new(),
            },
            Caption {
                id: "b".into(),
                start: 1.0,
                end: 2.0,
                english: "today".into(),
                hindi: String::new(),
            },
        ];
        let good = with_line_breaks(&caps, &["to save money\nthat works".into(), "today".into()]).unwrap();
        assert_eq!(good[0].english, "to save money\nthat works");
        assert_eq!(good[0].id, "a");
        assert!(with_line_breaks(&caps, &["to save money".into(), "today".into()]).is_none());
        assert!(with_line_breaks(&caps, &["today".into()]).is_none());
    }

    #[test]
    fn filler_phrases_show_nothing() {
        let words = vec![word("अम्म", 0.0, 0.5), word("हाँ", 1.0, 1.2)];
        let caps = build_captions(&words, &[phrase(0, 0, ""), phrase(1, 1, "Yes")]);
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].start, 1.0);
    }
}
