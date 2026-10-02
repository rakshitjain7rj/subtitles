//! Reading what a video file actually is, so the export can match it.

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{msg, Result};
use crate::ffmpeg::{run_capture, Tools};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Hdr {
    None,
    Pq,
    Hlg,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaInfo {
    /// Dimensions as displayed, i.e. after applying any rotation flag.
    pub width: u32,
    pub height: u32,
    pub rotation: i32,
    pub fps: f64,
    pub duration: f64,
    pub frame_count: Option<u64>,
    pub video_codec: String,
    pub pix_fmt: String,
    pub bit_depth: u8,
    pub color_transfer: Option<String>,
    pub color_primaries: Option<String>,
    pub color_space: Option<String>,
    pub color_range: Option<String>,
    pub hdr: Hdr,
    pub dolby_vision: bool,
    /// x265 `master-display` / `max-cll` strings, when the source carries them.
    pub master_display: Option<String>,
    pub max_cll: Option<String>,
    pub audio_codec: Option<String>,
    pub video_bitrate: Option<u64>,
    pub size_bytes: u64,
}

fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && *s != "unknown")
        .map(str::to_string)
}

fn num_field<T: std::str::FromStr>(v: &Value, key: &str) -> Option<T> {
    match v.get(key)? {
        Value::String(s) => s.parse().ok(),
        other => other.to_string().parse().ok(),
    }
}

/// "30000/1001" -> 29.97
pub fn parse_rational(s: &str) -> Option<f64> {
    let (n, d) = match s.split_once('/') {
        Some((n, d)) => (n.trim().parse::<f64>().ok()?, d.trim().parse::<f64>().ok()?),
        None => (s.trim().parse::<f64>().ok()?, 1.0),
    };
    (d != 0.0 && n.is_finite()).then(|| n / d)
}

fn bit_depth_of(pix_fmt: &str) -> u8 {
    if pix_fmt.starts_with("p010") {
        return 10;
    }
    for depth in [16u8, 14, 12, 10, 9] {
        if pix_fmt.contains(&format!("p{depth}")) {
            return depth;
        }
    }
    8
}

fn side_data(stream: &Value) -> impl Iterator<Item = &Value> {
    stream
        .get("side_data_list")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn rotation_of(stream: &Value) -> i32 {
    let from_side_data = side_data(stream).find_map(|sd| num_field::<f64>(sd, "rotation"));
    let from_tag = stream.get("tags").and_then(|t| num_field::<f64>(t, "rotate"));
    let deg = from_side_data.or(from_tag).unwrap_or(0.0).round() as i32;
    deg.rem_euclid(360)
}

/// Parses `ffprobe -show_format -show_streams -of json` output.
pub fn parse_probe(json: &str, size_bytes: u64) -> Result<MediaInfo> {
    let root: Value = serde_json::from_str(json)?;
    let streams = root
        .get("streams")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let is_type = |s: &Value, t: &str| s.get("codec_type").and_then(Value::as_str) == Some(t);
    let is_cover = |s: &Value| {
        s.get("disposition")
            .and_then(|d| d.get("attached_pic"))
            .and_then(Value::as_i64)
            == Some(1)
    };
    let video = streams
        .iter()
        .find(|s| is_type(s, "video") && !is_cover(s))
        .ok_or_else(|| msg("This file has no video stream."))?;
    let audio = streams.iter().find(|s| is_type(s, "audio"));
    let format = root.get("format").cloned().unwrap_or(Value::Null);

    let coded_w: u32 = num_field(video, "width").ok_or_else(|| msg("Video width is unknown."))?;
    let coded_h: u32 = num_field(video, "height").ok_or_else(|| msg("Video height is unknown."))?;
    let rotation = rotation_of(video);
    let (width, height) = if rotation == 90 || rotation == 270 {
        (coded_h, coded_w)
    } else {
        (coded_w, coded_h)
    };

    let fps = str_field(video, "avg_frame_rate")
        .and_then(|s| parse_rational(&s))
        .filter(|f| *f > 0.0)
        .or_else(|| str_field(video, "r_frame_rate").and_then(|s| parse_rational(&s)))
        .unwrap_or(0.0);
    let duration = num_field::<f64>(video, "duration")
        .or_else(|| num_field::<f64>(&format, "duration"))
        .unwrap_or(0.0);

    let pix_fmt = str_field(video, "pix_fmt").unwrap_or_else(|| "yuv420p".into());
    let color_transfer = str_field(video, "color_transfer");
    let hdr = match color_transfer.as_deref() {
        Some("smpte2084") => Hdr::Pq,
        Some("arib-std-b67") => Hdr::Hlg,
        _ => Hdr::None,
    };
    let dolby_vision = side_data(video).any(|sd| {
        sd.get("side_data_type")
            .and_then(Value::as_str)
            .is_some_and(|t| t.contains("DOVI"))
    });

    Ok(MediaInfo {
        width,
        height,
        rotation,
        fps,
        duration,
        frame_count: num_field(video, "nb_frames"),
        video_codec: str_field(video, "codec_name").unwrap_or_default(),
        bit_depth: bit_depth_of(&pix_fmt),
        pix_fmt,
        color_transfer,
        color_primaries: str_field(video, "color_primaries"),
        color_space: str_field(video, "color_space"),
        color_range: str_field(video, "color_range"),
        hdr,
        dolby_vision,
        master_display: None,
        max_cll: None,
        audio_codec: audio.and_then(|a| str_field(a, "codec_name")),
        video_bitrate: num_field(video, "bit_rate"),
        size_bytes,
    })
}

/// Parses the first frame's side data (`ffprobe -show_frames`) into the
/// `master-display` and `max-cll` strings x265 expects.
pub fn parse_hdr10_metadata(json: &str) -> (Option<String>, Option<String>) {
    let Ok(root) = serde_json::from_str::<Value>(json) else {
        return (None, None);
    };
    let Some(frame) = root.get("frames").and_then(Value::as_array).and_then(|f| f.first()) else {
        return (None, None);
    };
    let mut master = None;
    let mut cll = None;
    for sd in side_data(frame) {
        let kind = sd.get("side_data_type").and_then(Value::as_str).unwrap_or("");
        if kind.contains("Mastering display") {
            // x265 units: chromaticity in 0.00002, luminance in 0.0001 cd/m2.
            let chroma = |k: &str| {
                str_field(sd, k)
                    .and_then(|s| parse_rational(&s))
                    .map(|v| (v * 50000.0).round() as u64)
            };
            let lum = |k: &str| {
                str_field(sd, k)
                    .and_then(|s| parse_rational(&s))
                    .map(|v| (v * 10000.0).round() as u64)
            };
            let all = (|| {
                Some(format!(
                    "G({},{})B({},{})R({},{})WP({},{})L({},{})",
                    chroma("green_x")?,
                    chroma("green_y")?,
                    chroma("blue_x")?,
                    chroma("blue_y")?,
                    chroma("red_x")?,
                    chroma("red_y")?,
                    chroma("white_point_x")?,
                    chroma("white_point_y")?,
                    lum("max_luminance")?,
                    lum("min_luminance")?,
                ))
            })();
            master = all;
        } else if kind.contains("Content light level") {
            let max_content: Option<u64> = num_field(sd, "max_content");
            let max_average: Option<u64> = num_field(sd, "max_average");
            if let (Some(c), Some(a)) = (max_content, max_average) {
                cll = Some(format!("{c},{a}"));
            }
        }
    }
    (master, cll)
}

/// Counts video frames a player shows, from `ffprobe -show_entries
/// packet=flags -of csv=p=0` output. Packets flagged `D` (discard) are
/// encoder priming frames an MP4 edit list hides; screen recorders often
/// write a few, and the container's own frame count includes them.
pub fn count_shown_frames(packet_flags_csv: &str) -> u64 {
    packet_flags_csv
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.contains('D'))
        .count() as u64
}

pub async fn probe(tools: &Tools, path: &Path) -> Result<MediaInfo> {
    let size_bytes = tokio::fs::metadata(path).await?.len();
    let file = path.to_string_lossy().into_owned();
    let args: Vec<String> = ["-v", "error", "-of", "json", "-show_format", "-show_streams"]
        .iter()
        .map(|s| s.to_string())
        .chain([file.clone()])
        .collect();
    let json = run_capture(&tools.ffprobe, &args, None).await?;
    let mut info = parse_probe(&json, size_bytes)?;

    // Reads packet headers only (no decoding), so this is quick even for 4K.
    let args: Vec<String> = [
        "-v",
        "error",
        "-select_streams",
        "v:0",
        "-show_entries",
        "packet=flags",
        "-of",
        "csv=p=0",
    ]
    .iter()
    .map(|s| s.to_string())
    .chain([file.clone()])
    .collect();
    if let Ok(csv) = run_capture(&tools.ffprobe, &args, None).await {
        let shown = count_shown_frames(&csv);
        if shown > 0 {
            info.frame_count = Some(shown);
        }
    }

    if info.hdr == Hdr::Pq {
        let args: Vec<String> = [
            "-v",
            "error",
            "-of",
            "json",
            "-select_streams",
            "v:0",
            "-read_intervals",
            "%+#1",
            "-show_frames",
            "-show_entries",
            "frame=side_data_list",
        ]
        .iter()
        .map(|s| s.to_string())
        .chain([file])
        .collect();
        // Static metadata is optional; a clip without it is still valid HDR.
        if let Ok(json) = run_capture(&tools.ffprobe, &args, None).await {
            (info.master_display, info.max_cll) = parse_hdr10_metadata(&json);
        }
    }
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PHONE: &str = r#"{
      "streams": [
        {"codec_type":"video","codec_name":"hevc","width":3840,"height":2160,"pix_fmt":"yuv420p10le",
         "avg_frame_rate":"30000/1001","r_frame_rate":"30/1","duration":"12.5","nb_frames":"375","bit_rate":"45000000",
         "color_transfer":"arib-std-b67","color_primaries":"bt2020","color_space":"bt2020nc","color_range":"tv",
         "side_data_list":[{"side_data_type":"DOVI configuration record"},{"side_data_type":"Display Matrix","rotation":-90}]},
        {"codec_type":"audio","codec_name":"aac"}
      ],
      "format": {"duration":"12.6"}
    }"#;

    #[test]
    fn rotated_hlg_phone_clip() {
        let info = parse_probe(PHONE, 1000).unwrap();
        assert_eq!((info.width, info.height), (2160, 3840));
        assert_eq!(info.rotation, 270);
        assert_eq!(info.hdr, Hdr::Hlg);
        assert!(info.dolby_vision);
        assert_eq!(info.bit_depth, 10);
        assert!((info.fps - 29.97).abs() < 0.01);
        assert_eq!(info.frame_count, Some(375));
        assert_eq!(info.audio_codec.as_deref(), Some("aac"));
    }

    #[test]
    fn plain_sdr_clip_without_audio() {
        let json = r#"{"streams":[{"codec_type":"video","codec_name":"h264","width":1080,"height":1920,
            "pix_fmt":"yuv420p","avg_frame_rate":"30/1","color_transfer":"bt709"}],"format":{"duration":"3.0"}}"#;
        let info = parse_probe(json, 1).unwrap();
        assert_eq!(info.hdr, Hdr::None);
        assert_eq!(info.bit_depth, 8);
        assert_eq!(info.duration, 3.0);
        assert!(info.audio_codec.is_none());
    }

    #[test]
    fn audio_only_is_rejected() {
        let json = r#"{"streams":[{"codec_type":"audio","codec_name":"aac"}],"format":{}}"#;
        assert!(parse_probe(json, 1).is_err());
    }

    #[test]
    fn hidden_priming_frames_are_not_counted() {
        // The start of a real screen recording: 6 discarded packets, then video.
        let csv = "KD_\n_D_\n_D_\n_D_\n_D_\n_D_\nK__\n___\n___\n";
        assert_eq!(count_shown_frames(csv), 3);
        assert_eq!(count_shown_frames(""), 0);
    }

    #[test]
    fn hdr10_static_metadata() {
        let json = r#"{"frames":[{"side_data_list":[
          {"side_data_type":"Mastering display metadata","red_x":"34000/50000","red_y":"16000/50000",
           "green_x":"13250/50000","green_y":"34500/50000","blue_x":"7500/50000","blue_y":"3000/50000",
           "white_point_x":"15635/50000","white_point_y":"16450/50000","min_luminance":"50/10000","max_luminance":"10000000/10000"},
          {"side_data_type":"Content light level metadata","max_content":1000,"max_average":400}]}]}"#;
        let (master, cll) = parse_hdr10_metadata(json);
        assert_eq!(
            master.as_deref(),
            Some("G(13250,34500)B(7500,3000)R(34000,16000)WP(15635,16450)L(10000000,50)")
        );
        assert_eq!(cll.as_deref(), Some("1000,400"));
    }
}
