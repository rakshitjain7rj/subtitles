//! The one caption style, as an ASS script for ffmpeg's `ass` filter.
//! `src/lib/style.ts` mirrors these numbers for the live preview.

use serde::{Deserialize, Serialize};

use crate::captions::Caption;
use crate::probe::Hdr;

pub const FONT_FILE: &str = "Montserrat-ExtraBold.ttf";
pub const FONT_BYTES: &[u8] = include_bytes!("../resources/fonts/Montserrat-ExtraBold.ttf");
const FONT_NAME: &str = "Montserrat ExtraBold";
/// ASS font sizes measure the font's full line height (ascent + descent),
/// which for this font is 1.562 em.
const ASS_SIZE_PER_EM: f64 = 1.562;
const OUTLINE_PER_EM: f64 = 0.09;
const SIDE_MARGIN_FRACTION: f64 = 0.06;
/// Average glyph advance of the caption font, used to estimate wrapping.
const ADVANCE_PER_EM: f64 = 0.66;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Style {
    /// Vertical centre of the caption, percent of height from the top.
    pub y_pct: f64,
    /// Text size (em), percent of the video's shorter side.
    pub size_pct: f64,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            y_pct: 72.0,
            size_pct: 6.5,
        }
    }
}

impl Style {
    pub fn clamped(self) -> Style {
        Style {
            y_pct: self.y_pct.clamp(5.0, 95.0),
            size_pct: self.size_pct.clamp(2.5, 14.0),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub em: f64,
    pub outline: f64,
    pub margin_h: u32,
    pub center_x: u32,
    pub center_y: u32,
    /// Rows `band_top..band_bottom` may contain caption pixels.
    pub band_top: u32,
    pub band_bottom: u32,
}

/// `pre_wrapped` means the captions already carry their line breaks (the
/// review screen's own wrapping), so the line count is known exactly.
pub fn layout(width: u32, height: u32, style: Style, captions: &[Caption], pre_wrapped: bool) -> Layout {
    let style = style.clamped();
    let em = (style.size_pct / 100.0 * width.min(height) as f64).round().max(8.0);
    let outline = (em * OUTLINE_PER_EM * 10.0).round() / 10.0;
    let margin_h = (width as f64 * SIDE_MARGIN_FRACTION).round() as u32;
    let center_y = (style.y_pct / 100.0 * height as f64).round() as u32;

    let lines = if pre_wrapped {
        captions
            .iter()
            .map(|c| c.english.trim().lines().count())
            .max()
            .unwrap_or(1)
            .max(1) as f64
    } else {
        // Wrapping is estimated, so the band allows one line more than expected.
        let usable = (width - 2 * margin_h) as f64;
        let longest = captions.iter().map(|c| c.english.chars().count()).max().unwrap_or(0) as f64;
        ((longest * ADVANCE_PER_EM * em / usable).ceil() + 1.0).clamp(2.0, 5.0)
    };
    let half = lines * ASS_SIZE_PER_EM * em / 2.0 + outline + em * 0.1;
    let band_top = (center_y as f64 - half).floor().max(0.0) as u32;
    let band_bottom = ((center_y as f64 + half).ceil() as u32).min(height);

    Layout {
        em,
        outline,
        margin_h,
        center_x: width / 2,
        center_y,
        band_top,
        band_bottom,
    }
}

/// White in an HDR frame would be rendered at the signal's peak, far brighter
/// than anything else on screen, so captions use each curve's reference white.
fn text_colour(hdr: Hdr) -> &'static str {
    match hdr {
        Hdr::None => "&H00FFFFFF",
        Hdr::Pq => "&H00949494",  // 58% signal, about 203 nits
        Hdr::Hlg => "&H00BFBFBF", // 75% signal
    }
}

fn timestamp(secs: f64) -> String {
    let cs = (secs.max(0.0) * 100.0).round() as u64;
    format!(
        "{}:{:02}:{:02}.{:02}",
        cs / 360_000,
        cs / 6000 % 60,
        cs / 100 % 60,
        cs % 100
    )
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.trim().chars() {
        match ch {
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            // A word joiner stops "\N", "\h" etc. typed by the user acting as tags.
            '\\' => out.push_str("\\\u{2060}"),
            '\n' => out.push_str("\\N"),
            '\r' => {}
            other => out.push(other),
        }
    }
    out
}

/// With `pre_wrapped`, line breaks in the text are the only ones used
/// (WrapStyle 2); otherwise libass wraps long captions itself.
pub fn render(width: u32, height: u32, layout: &Layout, captions: &[Caption], hdr: Hdr, pre_wrapped: bool) -> String {
    let font_size = (layout.em * ASS_SIZE_PER_EM * 10.0).round() / 10.0;
    let mut s = String::new();
    s.push_str("[Script Info]\nScriptType: v4.00+\n");
    s.push_str(&format!("PlayResX: {width}\nPlayResY: {height}\n"));
    s.push_str(&format!("WrapStyle: {}\n", if pre_wrapped { 2 } else { 0 }));
    s.push_str("ScaledBorderAndShadow: yes\nYCbCr Matrix: None\n\n");
    s.push_str("[V4+ Styles]\n");
    s.push_str("Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n");
    s.push_str(&format!(
        "Style: Caption,{FONT_NAME},{font_size},{colour},{colour},&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,{outline},0,5,{m},{m},0,1\n\n",
        colour = text_colour(hdr),
        outline = layout.outline,
        m = layout.margin_h,
    ));
    s.push_str("[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n");
    for c in captions {
        let text = escape(&c.english);
        if text.is_empty() || c.end <= c.start {
            continue;
        }
        let start = timestamp(c.start);
        let mut end = timestamp(c.end);
        if end == start {
            end = timestamp(c.start + 0.01);
        }
        s.push_str(&format!(
            "Dialogue: 0,{start},{end},Caption,,0,0,0,,{{\\pos({},{})}}{text}\n",
            layout.center_x, layout.center_y
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caption(start: f64, end: f64, english: &str) -> Caption {
        Caption {
            id: "x".into(),
            start,
            end,
            english: english.into(),
            hindi: String::new(),
        }
    }

    #[test]
    fn timestamps_round_to_centiseconds() {
        assert_eq!(timestamp(0.0), "0:00:00.00");
        assert_eq!(timestamp(61.239), "0:01:01.24");
        assert_eq!(timestamp(3600.0), "1:00:00.00");
    }

    #[test]
    fn layout_scales_with_the_shorter_side() {
        let portrait = layout(1080, 1920, Style::default(), &[], false);
        let landscape = layout(1920, 1080, Style::default(), &[], false);
        assert_eq!(portrait.em, 70.0);
        assert_eq!(landscape.em, 70.0);
        assert_eq!(portrait.center_y, 1382);
        assert!(portrait.band_top < portrait.center_y && portrait.center_y < portrait.band_bottom);
    }

    #[test]
    fn band_grows_for_long_captions_and_stays_in_frame() {
        let short = layout(1080, 1920, Style::default(), &[caption(0.0, 1.0, "Hi there")], false);
        let long = layout(
            1080,
            1920,
            Style::default(),
            &[caption(0.0, 1.0, &"word ".repeat(12))],
            false,
        );
        assert!(long.band_bottom - long.band_top > short.band_bottom - short.band_top);
        let low = layout(
            1080,
            1920,
            Style {
                y_pct: 95.0,
                size_pct: 14.0,
            },
            &[],
            false,
        );
        assert_eq!(low.band_bottom, 1920);
    }

    #[test]
    fn script_escapes_text_and_skips_empty_captions() {
        let l = layout(1080, 1920, Style::default(), &[], false);
        let caps = [
            caption(1.0, 2.0, "a {b}\\N c"),
            caption(2.0, 3.0, "  "),
            caption(3.0, 3.0, "zero length"),
        ];
        let script = render(1080, 1920, &l, &caps, Hdr::None, false);
        assert!(script.contains("PlayResX: 1080"));
        assert!(script.contains("{\\pos(540,1382)}a \\{b\\}\\\u{2060}N c"));
        assert_eq!(script.matches("Dialogue:").count(), 1);
        assert!(script.contains("WrapStyle: 0"));
        assert!(render(1080, 1920, &l, &caps, Hdr::Pq, false).contains("&H00949494"));
    }

    #[test]
    fn pre_wrapped_captions_keep_their_breaks_and_size_the_band_exactly() {
        let caps = [
            caption(0.0, 1.0, "to save money\nthat really works"),
            caption(1.0, 2.0, "today"),
        ];
        let exact = layout(1080, 1920, Style::default(), &caps, true);
        let guessed = layout(1080, 1920, Style::default(), &caps, false);
        // Two real lines, against the estimate's spare third.
        assert!(exact.band_bottom - exact.band_top < guessed.band_bottom - guessed.band_top);
        let script = render(1080, 1920, &exact, &caps, Hdr::None, true);
        assert!(script.contains("WrapStyle: 2"));
        assert!(script.contains("to save money\\Nthat really works"));
    }
}
