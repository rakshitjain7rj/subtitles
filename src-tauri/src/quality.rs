//! Proof that the export matches the source: VMAF measured on the parts of
//! the picture the captions don't touch, plus a like-for-like spec check.

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::encode::Quality;
use crate::error::{msg, Result};
use crate::probe::{Hdr, MediaInfo};

pub const VMAF_LOG: &str = "vmaf.json";
pub const CEILING_LOG: &str = "vmaf-ceiling.json";
/// Beyond this many frames, VMAF scores every Nth frame to keep the check quick.
const FULL_SCORE_FRAMES: u64 = 600;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VmafStats {
    pub mean: f64,
    pub min: f64,
    pub harmonic_mean: f64,
    /// Luma PSNR in dB; libvmaf reports 60 for identical frames.
    #[serde(default)]
    pub psnr_y: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityReport {
    pub vmaf: Option<VmafStats>,
    /// VMAF of the source against itself over the same area and frames: the
    /// best score this video can get. It is often below 100, especially on
    /// low-motion video such as screen recordings.
    #[serde(default)]
    pub ceiling: Option<f64>,
    #[serde(default)]
    pub quality: Quality,
    /// Share of the frame VMAF was measured on (the rest is the caption band).
    pub measured_fraction: f64,
    /// 1 = every frame scored, 3 = every third frame, and so on.
    pub frame_step: u32,
    pub resolution_match: bool,
    pub fps_match: bool,
    pub frame_count_match: Option<bool>,
    pub audio_match: bool,
    pub hdr_match: bool,
    pub source: MediaInfo,
    pub export: MediaInfo,
    pub notes: Vec<String>,
}

/// The rows to measure, as `(y, height)`: whatever lies above and below the
/// caption band, skipping slivers too thin for VMAF to say anything about.
pub fn measured_bands(height: u32, band_top: u32, band_bottom: u32) -> Vec<(u32, u32)> {
    let min_rows = (height / 10).max(128);
    let even_down = |v: u32| v & !1;
    let mut bands = Vec::new();
    let above = even_down(band_top.min(height));
    if above >= min_rows {
        bands.push((0, above));
    }
    let below_start = (band_bottom.min(height) + 1) & !1;
    let below = even_down(height.saturating_sub(below_start));
    if below >= min_rows {
        bands.push((below_start, below));
    }
    bands
}

pub fn frame_step(frame_count: Option<u64>) -> u32 {
    frame_count.map_or(1, |n| n.div_ceil(FULL_SCORE_FRAMES).max(1) as u32)
}

/// ffmpeg arguments comparing `export` against `source` over `bands`, and,
/// in the same pass, `source` against itself for the ceiling. Run in the
/// working directory; the scores land in `VMAF_LOG` and `CEILING_LOG`.
pub fn vmaf_args(
    source: &Path,
    export: &Path,
    info: &MediaInfo,
    bands: &[(u32, u32)],
    step: u32,
    threads: usize,
) -> Vec<String> {
    let pix_fmt = if info.bit_depth > 8 { "yuv420p10le" } else { "yuv420p" };
    let w = info.width & !1;
    let region = |input: usize, label: &str| -> String {
        let head = format!("[{input}:v]setpts=PTS-STARTPTS,format={pix_fmt}");
        match bands {
            [(y, h)] => format!("{head},crop={w}:{h}:0:{y}[{label}]"),
            [(y1, h1), (y2, h2)] => format!(
                "{head},split[{label}a][{label}b];[{label}a]crop={w}:{h1}:0:{y1}[{label}a1];\
                 [{label}b]crop={w}:{h2}:0:{y2}[{label}b1];[{label}a1][{label}b1]vstack[{label}]"
            ),
            _ => format!("{head}[{label}]"),
        }
    };
    let opts = format!("log_fmt=json:n_threads={threads}:n_subsample={step}");
    let graph = format!(
        "{};{};[ref]split=3[r0][r1][r2];\
         [dist][r0]libvmaf={opts}:log_path={VMAF_LOG}:feature=name=psnr[scored];\
         [r1][r2]libvmaf={opts}:log_path={CEILING_LOG}[ceiling]",
        region(0, "dist"),
        region(1, "ref"),
    );
    let mut args: Vec<String> = vec![
        "-i".into(),
        export.to_string_lossy().into_owned(),
        "-i".into(),
        source.to_string_lossy().into_owned(),
        "-filter_complex".into(),
        graph,
    ];
    for out in ["[scored]", "[ceiling]"] {
        args.extend(["-map", out, "-f", "null", "-"].map(String::from));
    }
    args
}

pub fn parse_vmaf_log(json: &str) -> Result<VmafStats> {
    let root: Value = serde_json::from_str(json)?;
    let pooled = root
        .get("pooled_metrics")
        .and_then(|p| p.get("vmaf"))
        .ok_or_else(|| msg("The VMAF log has no pooled score."))?;
    let get = |k: &str| {
        pooled
            .get(k)
            .and_then(Value::as_f64)
            .ok_or_else(|| msg(format!("VMAF log is missing {k}.")))
    };
    let psnr_y = root
        .get("pooled_metrics")
        .and_then(|p| p.get("psnr_y"))
        .and_then(|p| p.get("mean"))
        .and_then(Value::as_f64);
    Ok(VmafStats {
        mean: get("mean")?,
        min: get("min")?,
        harmonic_mean: get("harmonic_mean")?,
        psnr_y,
    })
}

/// What `measure` in the pipeline found.
pub struct Measurement {
    pub vmaf: Option<VmafStats>,
    pub ceiling: Option<f64>,
    /// Share of the frame measured.
    pub fraction: f64,
    pub step: u32,
    pub notes: Vec<String>,
}

pub fn build_report(source: MediaInfo, export: MediaInfo, quality: Quality, m: Measurement) -> QualityReport {
    let Measurement {
        vmaf,
        ceiling,
        fraction: measured_fraction,
        step: frame_step,
        mut notes,
    } = m;
    let frame_count_match = match (source.frame_count, export.frame_count) {
        (Some(a), Some(b)) => Some(a == b),
        _ => None,
    };
    if source.hdr != Hdr::None && vmaf.is_some() {
        notes.push(
            "VMAF is calibrated on SDR video, so on HDR it is a fidelity check rather than an exact score.".into(),
        );
    }
    if source.dolby_vision {
        notes.push("The source's Dolby Vision layer is not carried over; the export plays as standard HDR.".into());
    }
    QualityReport {
        vmaf,
        ceiling,
        quality,
        measured_fraction,
        frame_step,
        resolution_match: source.width == export.width && source.height == export.height,
        fps_match: (source.fps - export.fps).abs() < 0.01,
        frame_count_match,
        audio_match: source.audio_codec == export.audio_codec,
        hdr_match: source.hdr == export.hdr,
        source,
        export,
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_surround_the_captions() {
        // Captions low in a portrait frame: a big band above, a smaller one below.
        assert_eq!(measured_bands(1920, 1201, 1563), vec![(0, 1200), (1564, 356)]);
        // Captions at the very bottom: only the area above is measured.
        assert_eq!(measured_bands(1080, 850, 1080), vec![(0, 850)]);
        // A thin strip above the captions is ignored.
        assert_eq!(measured_bands(1080, 60, 300), vec![(300, 780)]);
    }

    #[test]
    fn long_clips_are_subsampled() {
        assert_eq!(frame_step(Some(300)), 1);
        assert_eq!(frame_step(Some(600)), 1);
        assert_eq!(frame_step(Some(1801)), 4);
        assert_eq!(frame_step(None), 1);
    }

    #[test]
    fn reads_pooled_scores() {
        let log = r#"{"frames":[],"pooled_metrics":{"vmaf":{"min":93.1,"max":99.9,"mean":97.4,"harmonic_mean":97.3}}}"#;
        assert_eq!(
            parse_vmaf_log(log).unwrap(),
            VmafStats {
                mean: 97.4,
                min: 93.1,
                harmonic_mean: 97.3,
                psnr_y: None,
            }
        );
        assert!(parse_vmaf_log("{}").is_err());
        let with_psnr = r#"{"pooled_metrics":{"vmaf":{"min":1,"mean":2,"harmonic_mean":3},"psnr_y":{"mean":54.2}}}"#;
        assert_eq!(parse_vmaf_log(with_psnr).unwrap().psnr_y, Some(54.2));
    }

    #[test]
    fn reports_saved_before_the_ceiling_existed_still_load() {
        let old = r#"{"mean":97.2,"min":96.7,"harmonic_mean":97.2}"#;
        assert_eq!(serde_json::from_str::<VmafStats>(old).unwrap().psnr_y, None);
    }
}
