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

/// The most English words a caption shows at once.
pub const MAX_PHRASE_WORDS: usize = 5;

/// Words a caption reads badly ending on ("the | founder").
const NO_BREAK_AFTER: &[&str] = &[
    "a", "an", "the", "to", "of", "my", "your", "his", "her", "its", "our", "their", "this", "that's",
    "very", "not", "so", "is", "was", "are", "were", "be", "i", "you", "he", "she", "we", "they",
];
/// Words a new caption reads well starting with ("| of the Sikh Empire").
const GOOD_BREAK_BEFORE: &[&str] = &[
    "and", "but", "or", "so", "because", "that", "which", "who", "when", "where", "if", "then", "of",
    "from", "to", "in", "on", "at", "with", "for", "about", "like",
];

/// Splits phrases longer than [`MAX_PHRASE_WORDS`] at the most natural
/// boundaries and shares their spoken words out in proportion, so the limit
/// holds whatever the translator returned. A phrase spoken over too few
/// words to give each piece one of its own is split into as many as it can.
pub fn split_long_phrases(phrases: Vec<Phrase>) -> Vec<Phrase> {
    let mut out = Vec::with_capacity(phrases.len());
    for p in phrases {
        let english: Vec<&str> = p.english.split_whitespace().collect();
        let spoken = p.last - p.first + 1;
        let pieces = english.len().div_ceil(MAX_PHRASE_WORDS).min(spoken);
        if pieces < 2 {
            out.push(p);
            continue;
        }
        let breaks = best_breaks(&english, pieces);
        let mut start = 0;
        let mut first = p.first;
        for (i, &end) in breaks.iter().chain([english.len()].iter()).enumerate() {
            // Spoken words up to this piece's end, in proportion, leaving at
            // least one for each piece still to come.
            let remaining = pieces - i - 1;
            let last = if remaining == 0 {
                p.last
            } else {
                let share = (spoken * end + english.len() / 2) / english.len();
                (p.first + share).saturating_sub(1).clamp(first, p.last - remaining)
            };
            out.push(Phrase {
                first,
                last,
                english: english[start..end].join(" "),
            });
            start = end;
            first = last + 1;
        }
    }
    out
}

/// Where to cut `words` into `pieces` runs: the indices each later piece
/// starts at. Prefers even lengths, cuts after punctuation or before
/// conjunctions and prepositions, and avoids one-word pieces and cuts after
/// articles.
fn best_breaks(words: &[&str], pieces: usize) -> Vec<usize> {
    let n = words.len();
    let max_len = n.div_ceil(pieces).max(MAX_PHRASE_WORDS);
    let even = n as f64 / pieces as f64;
    let bare = |w: &str| {
        w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'')
            .to_lowercase()
    };
    let cut_cost = |at: usize| -> f64 {
        if at == n {
            return 0.0;
        }
        let before = words[at - 1];
        let mut cost = 0.0;
        if before.ends_with([',', '.', '?', '!', ';', ':', '—']) {
            cost -= 3.0;
        }
        if GOOD_BREAK_BEFORE.contains(&bare(words[at]).as_str()) {
            cost -= 1.5;
        }
        if NO_BREAK_AFTER.contains(&bare(before).as_str()) {
            cost += 3.0;
        }
        cost
    };
    let piece_cost = |len: usize| -> f64 {
        let lonely = if len == 1 { 2.0 } else { 0.0 };
        lonely + (len as f64 - even).powi(2) * 0.5
    };

    // cost[k][i]: best cost of cutting words[..i] into k pieces.
    let mut cost = vec![vec![f64::INFINITY; n + 1]; pieces + 1];
    let mut from = vec![vec![0; n + 1]; pieces + 1];
    cost[0][0] = 0.0;
    for k in 1..=pieces {
        for i in k..=n {
            for len in 1..=max_len.min(i) {
                let prev = cost[k - 1][i - len];
                let c = prev + piece_cost(len) + cut_cost(i);
                if c < cost[k][i] {
                    cost[k][i] = c;
                    from[k][i] = i - len;
                }
            }
        }
    }
    let mut breaks = Vec::with_capacity(pieces - 1);
    let mut i = n;
    for k in (2..=pieces).rev() {
        i = from[k][i];
        breaks.push(i);
    }
    breaks.reverse();
    breaks
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

    fn texts(phrases: &[Phrase]) -> Vec<&str> {
        phrases.iter().map(|p| p.english.as_str()).collect()
    }

    #[test]
    fn long_phrases_split_at_natural_points() {
        let split = split_long_phrases(vec![
            phrase(0, 1, "Today"),
            phrase(2, 9, "He was basically the founder of the Sikh Empire"),
            phrase(10, 13, "So he used to say, he weighed them all"),
        ]);
        assert_eq!(
            texts(&split),
            [
                "Today",
                "He was basically the founder",
                "of the Sikh Empire",
                "So he used to say,",
                "he weighed them all",
            ]
        );
        // Spoken words are shared out in order with none skipped.
        let ranges: Vec<_> = split.iter().map(|p| (p.first, p.last)).collect();
        assert_eq!(ranges, [(0, 1), (2, 5), (6, 9), (10, 11), (12, 13)]);
    }

    #[test]
    fn every_piece_keeps_the_limit_and_a_spoken_word() {
        let english = "one two three four five six seven eight nine ten eleven twelve";
        for spoken in 1..8 {
            let split = split_long_phrases(vec![phrase(4, 3 + spoken, english)]);
            assert_eq!(split.first().unwrap().first, 4);
            assert_eq!(split.last().unwrap().last, 3 + spoken);
            assert_eq!(split.join_english(), english);
            for pair in split.windows(2) {
                assert_eq!(pair[0].last + 1, pair[1].first);
            }
            assert!(split.iter().all(|p| p.first <= p.last));
            if spoken >= 3 {
                assert!(split.iter().all(|p| p.english.split(' ').count() <= MAX_PHRASE_WORDS));
            }
        }
    }

    trait JoinEnglish {
        fn join_english(&self) -> String;
    }

    impl JoinEnglish for Vec<Phrase> {
        fn join_english(&self) -> String {
            texts(self).join(" ")
        }
    }

    #[test]
    fn filler_phrases_show_nothing() {
        let words = vec![word("अम्म", 0.0, 0.5), word("हाँ", 1.0, 1.2)];
        let caps = build_captions(&words, &[phrase(0, 0, ""), phrase(1, 1, "Yes")]);
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].start, 1.0);
    }
}
