//! ffmpeg argument lists: audio for transcription, the preview proxy, and the
//! final encode. The final encode never changes resolution, frame timing or
//! audio; it only re-compresses the picture, at a quality meant to be
//! indistinguishable from the source.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::probe::{Hdr, MediaInfo};

pub const ASS_FILE: &str = "captions.ass";
pub const FONTS_DIR: &str = "fonts";

// CRF 12 was chosen by measuring a real 1080p60 screen recording: it closed
// most of the gap to the VMAF of the source against itself (97.30 against
// 97.43, where CRF 16 gave 97.15) at about the source's own file size.
const X264_CRF: &str = "12";
const X264_PRESET: &str = "fast";
const X265_CRF: &str = "12";
const X265_PRESET: &str = "medium";

/// How hard the export tries to preserve the picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    /// Visually identical at a sensible size; plays everywhere.
    #[default]
    High,
    /// Every pixel outside the captions exactly as the source decodes.
    /// Files are several times larger, and lossless H.264 needs a player
    /// that supports the High 4:4:4 Predictive profile.
    Lossless,
}
const PREVIEW_MAX_HEIGHT: u32 = 720;

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}

fn path_arg(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// Mono 16 kHz FLAC: small to upload, lossless for the speech model.
pub fn audio_args(src: &Path, out: &Path) -> Vec<String> {
    let mut a = strings(&["-y", "-i"]);
    a.push(path_arg(src));
    a.extend(strings(&[
        "-vn", "-map", "0:a:0", "-ac", "1", "-ar", "16000", "-c:a", "flac",
    ]));
    a.push(path_arg(out));
    a
}

/// One small JPEG frame from early in the video, for the project list.
pub fn thumbnail_args(info: &MediaInfo, src: &Path, out: &Path) -> Vec<String> {
    let at = (info.duration / 2.0).min(1.0);
    let mut a = strings(&["-y", "-ss"]);
    a.push(format!("{at:.3}"));
    a.push("-i".into());
    a.push(path_arg(src));
    a.extend(strings(&["-frames:v", "1", "-vf", "scale=-2:240", "-q:v", "4"]));
    a.push(path_arg(out));
    a
}

/// Whether the webviews can play `src` as it is, so no preview copy has to be
/// made: 8-bit SDR H.264 up to 1080p60 with AAC or MP3 audio, in MP4 or MOV.
pub fn plays_directly(info: &MediaInfo, src: &Path) -> bool {
    let container = src
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "mp4" | "m4v" | "mov"));
    container
        && info.video_codec == "h264"
        && matches!(info.pix_fmt.as_str(), "yuv420p" | "yuvj420p")
        && info.bit_depth == 8
        && info.hdr == Hdr::None
        && !info.dolby_vision
        && info.width.min(info.height) <= 1080
        && info.width.max(info.height) <= 1920
        && info.fps <= 60.5
        && info.audio_codec.as_deref().is_none_or(|a| matches!(a, "aac" | "mp3"))
}

/// A small H.264 copy that every webview can play. `tonemap` converts HDR to
/// SDR so the preview isn't washed out; it needs ffmpeg's zscale filter.
pub fn preview_args(info: &MediaInfo, src: &Path, out: &Path, tonemap: bool) -> Vec<String> {
    let mut filters: Vec<String> = Vec::new();
    if tonemap && info.hdr != Hdr::None {
        filters.push(
            "zscale=t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,tonemap=hable:desat=0,zscale=t=bt709:m=bt709:r=tv"
                .into(),
        );
    }
    if info.height > PREVIEW_MAX_HEIGHT {
        filters.push(format!("scale=-2:{PREVIEW_MAX_HEIGHT}"));
    }
    filters.push("format=yuv420p".into());

    let mut a = strings(&["-y", "-i"]);
    a.push(path_arg(src));
    a.extend(strings(&["-map", "0:v:0", "-map", "0:a:0?", "-vf"]));
    a.push(filters.join(","));
    a.extend(strings(&[
        "-c:v",
        "libx264",
        "-preset",
        "veryfast",
        "-crf",
        "24",
        "-g",
        "30",
        "-fps_mode",
        "passthrough",
        "-c:a",
        "aac",
        "-b:a",
        "128k",
        "-ac",
        "2",
        "-movflags",
        "+faststart",
    ]));
    a.push(path_arg(out));
    a
}

/// True when the export must be 10-bit HEVC: HDR, or a source deeper than
/// 8 bits that H.264 would have to truncate.
pub fn uses_hevc(info: &MediaInfo) -> bool {
    info.hdr != Hdr::None || info.bit_depth > 8
}

/// Container for the export: the source's own where we can, so the copied
/// audio stream is guaranteed to fit.
pub fn output_extension(src: &Path, info: &MediaInfo) -> &'static str {
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let pcm_audio = info.audio_codec.as_deref().is_some_and(|c| c.starts_with("pcm_"));
    match ext.as_str() {
        "mov" | "qt" => "mov",
        "mkv" | "webm" | "avi" | "ts" | "mts" | "m2ts" | "wmv" | "flv" => "mkv",
        _ if pcm_audio => "mov",
        _ => "mp4",
    }
}

/// A `setparams` filter that stamps the source's colour description on every
/// frame. The encoder takes its colour signalling from the frames, so this is
/// what makes players interpret the export the way they did the source.
fn colour_tags(info: &MediaInfo) -> String {
    let tags: Vec<String> = [
        ("color_primaries", &info.color_primaries),
        ("color_trc", &info.color_transfer),
        ("colorspace", &info.color_space),
        ("range", &info.color_range),
    ]
    .into_iter()
    .filter_map(|(key, value)| value.as_ref().map(|v| format!("{key}={v}")))
    .collect();
    if tags.is_empty() {
        String::new()
    } else {
        format!(",setparams={}", tags.join(":"))
    }
}

/// The final encode. Run with the working directory set to the folder that
/// holds `ASS_FILE` and `FONTS_DIR`, so the filter needs no path escaping.
pub fn export_args(info: &MediaInfo, src: &Path, out: &Path, quality: Quality) -> Vec<String> {
    let mp4_like = matches!(out.extension().and_then(|e| e.to_str()), Some("mp4" | "mov"));
    let mut a = strings(&["-y", "-i"]);
    a.push(path_arg(src));
    a.extend(strings(&["-map", "0:v:0", "-map", "0:a?", "-map_metadata", "0", "-vf"]));
    a.push(format!("ass={ASS_FILE}:fontsdir={FONTS_DIR}{}", colour_tags(info)));
    // Keep every frame at its original timestamp, including variable frame rate.
    a.extend(strings(&["-fps_mode", "passthrough"]));

    if uses_hevc(info) {
        a.extend(strings(&["-c:v", "libx265", "-preset", X265_PRESET]));
        if quality == Quality::High {
            a.extend(strings(&["-crf", X265_CRF]));
        }
        a.extend(strings(&["-pix_fmt", "yuv420p10le"]));
        if mp4_like {
            // Apple players only recognise HEVC in MP4/MOV under this tag.
            a.extend(strings(&["-tag:v", "hvc1"]));
        }
        let mut params = vec!["log-level=error".to_string(), "repeat-headers=1".to_string()];
        if quality == Quality::Lossless {
            params.push("lossless=1".into());
        }
        if info.hdr == Hdr::Pq {
            params.push("hdr10-opt=1".into());
            if let Some(md) = &info.master_display {
                params.push(format!("master-display={md}"));
            }
            if let Some(cll) = &info.max_cll {
                params.push(format!("max-cll={cll}"));
            }
        }
        a.push("-x265-params".into());
        a.push(params.join(":"));
    } else {
        let pix_fmt = if info.pix_fmt == "yuvj420p" {
            "yuvj420p"
        } else {
            "yuv420p"
        };
        a.extend(strings(&["-c:v", "libx264", "-preset", X264_PRESET]));
        match quality {
            Quality::High => a.extend(strings(&["-crf", X264_CRF])),
            Quality::Lossless => a.extend(strings(&["-qp", "0"])),
        }
        a.extend(strings(&["-pix_fmt", pix_fmt]));
    }

    a.extend(strings(&["-c:a", "copy"]));
    if mp4_like {
        a.extend(strings(&["-movflags", "+faststart"]));
    }
    a.push(path_arg(out));
    a
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn info(hdr: Hdr, pix_fmt: &str, bit_depth: u8) -> MediaInfo {
        MediaInfo {
            width: 1080,
            height: 1920,
            rotation: 0,
            fps: 30.0,
            duration: 10.0,
            frame_count: Some(300),
            video_codec: "h264".into(),
            pix_fmt: pix_fmt.into(),
            bit_depth,
            color_transfer: match hdr {
                Hdr::None => Some("bt709".into()),
                Hdr::Pq => Some("smpte2084".into()),
                Hdr::Hlg => Some("arib-std-b67".into()),
            },
            color_primaries: None,
            color_space: None,
            color_range: None,
            hdr,
            dolby_vision: false,
            master_display: Some("G(1,2)B(3,4)R(5,6)WP(7,8)L(9,10)".into()),
            max_cll: Some("1000,400".into()),
            audio_codec: Some("aac".into()),
            video_bitrate: None,
            size_bytes: 0,
        }
    }

    fn has_pair(args: &[String], flag: &str, value: &str) -> bool {
        args.windows(2).any(|w| w[0] == flag && w[1] == value)
    }

    #[test]
    fn sdr_is_h264_with_copied_audio_and_untouched_timing() {
        let a = export_args(
            &info(Hdr::None, "yuv420p", 8),
            &PathBuf::from("in.mp4"),
            &PathBuf::from("out.mp4"),
            Quality::High,
        );
        assert!(has_pair(&a, "-c:v", "libx264"));
        assert!(has_pair(&a, "-c:a", "copy"));
        assert!(has_pair(&a, "-fps_mode", "passthrough"));
        assert!(a
            .iter()
            .any(|s| s == "ass=captions.ass:fontsdir=fonts,setparams=color_trc=bt709"));
        assert!(!a.iter().any(|s| s.contains("scale")));
    }

    #[test]
    fn hdr_stays_hdr_in_ten_bit_hevc() {
        let a = export_args(
            &info(Hdr::Pq, "yuv420p10le", 10),
            &PathBuf::from("in.mov"),
            &PathBuf::from("out.mov"),
            Quality::High,
        );
        assert!(has_pair(&a, "-c:v", "libx265"));
        assert!(has_pair(&a, "-pix_fmt", "yuv420p10le"));
        assert!(a.iter().any(|s| s.ends_with("setparams=color_trc=smpte2084")));
        let params = &a[a.iter().position(|s| s == "-x265-params").unwrap() + 1];
        assert!(
            params.contains("hdr10-opt=1")
                && params.contains("master-display=G(1,2)")
                && params.contains("max-cll=1000,400")
        );

        let hlg = export_args(
            &info(Hdr::Hlg, "yuv420p10le", 10),
            &PathBuf::from("a.mov"),
            &PathBuf::from("b.mov"),
            Quality::High,
        );
        assert!(!hlg.iter().any(|s| s.contains("hdr10-opt")));
    }

    #[test]
    fn lossless_drops_rate_control_for_exact_coding() {
        let sdr = export_args(
            &info(Hdr::None, "yuv420p", 8),
            &PathBuf::from("a.mp4"),
            &PathBuf::from("b.mp4"),
            Quality::Lossless,
        );
        assert!(has_pair(&sdr, "-qp", "0"));
        assert!(!sdr.iter().any(|s| s == "-crf"));
        assert!(has_pair(
            &export_args(
                &info(Hdr::None, "yuv420p", 8),
                &PathBuf::from("a.mp4"),
                &PathBuf::from("b.mp4"),
                Quality::High,
            ),
            "-crf",
            "12"
        ));

        let hdr = export_args(
            &info(Hdr::Pq, "yuv420p10le", 10),
            &PathBuf::from("a.mov"),
            &PathBuf::from("b.mov"),
            Quality::Lossless,
        );
        assert!(!hdr.iter().any(|s| s == "-crf"));
        let params = &hdr[hdr.iter().position(|s| s == "-x265-params").unwrap() + 1];
        assert!(params.contains("lossless=1"));
    }

    #[test]
    fn ten_bit_sdr_is_not_truncated_to_eight() {
        assert!(uses_hevc(&info(Hdr::None, "yuv420p10le", 10)));
        assert!(!uses_hevc(&info(Hdr::None, "yuv420p", 8)));
    }

    #[test]
    fn container_follows_the_source() {
        let sdr = info(Hdr::None, "yuv420p", 8);
        assert_eq!(output_extension(&PathBuf::from("a.MOV"), &sdr), "mov");
        assert_eq!(output_extension(&PathBuf::from("a.mp4"), &sdr), "mp4");
        assert_eq!(output_extension(&PathBuf::from("a.mkv"), &sdr), "mkv");
        let mut pcm = sdr.clone();
        pcm.audio_codec = Some("pcm_s16le".into());
        assert_eq!(output_extension(&PathBuf::from("a.mp4"), &pcm), "mov");
    }

    #[test]
    fn typical_phone_and_screen_videos_play_without_a_preview_copy() {
        let mut reel = info(Hdr::None, "yuv420p", 8);
        reel.width = 1080;
        reel.height = 1920;
        reel.fps = 30.0;
        reel.audio_codec = Some("aac".into());
        assert!(plays_directly(&reel, Path::new("reel.MP4")));
        assert!(plays_directly(&reel, Path::new("reel.mov")));
        assert!(!plays_directly(&reel, Path::new("reel.mkv")));

        let iphone = MediaInfo {
            video_codec: "hevc".into(),
            ..reel.clone()
        };
        assert!(!plays_directly(&iphone, Path::new("reel.mov")));
        let four_k = MediaInfo {
            width: 2160,
            height: 3840,
            ..reel.clone()
        };
        assert!(!plays_directly(&four_k, Path::new("reel.mp4")));
        let hdr = info(Hdr::Pq, "yuv420p10le", 10);
        assert!(!plays_directly(&hdr, Path::new("reel.mp4")));
    }

    #[test]
    fn preview_tonemaps_hdr_only() {
        let hdr = preview_args(
            &info(Hdr::Hlg, "yuv420p10le", 10),
            &PathBuf::from("a"),
            &PathBuf::from("b"),
            true,
        );
        assert!(hdr.iter().any(|s| s.contains("tonemap")));
        let sdr = preview_args(
            &info(Hdr::None, "yuv420p", 8),
            &PathBuf::from("a"),
            &PathBuf::from("b"),
            true,
        );
        assert!(!sdr.iter().any(|s| s.contains("tonemap")));
    }
}
