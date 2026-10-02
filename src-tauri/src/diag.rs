//! A small log of what the app did and how it went, for testers to paste
//! when something goes wrong. It records steps, timings, video properties
//! and error messages; never API keys, transcripts or captions.

use std::fs::OpenOptions;
use std::future::Future;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::error::Result;
use crate::probe::MediaInfo;
use crate::quality::QualityReport;

/// Past this the oldest half is dropped.
const MAX_BYTES: usize = 256 * 1024;
/// Lines included in a copied report.
const REPORT_LINES: usize = 300;

pub struct Log {
    path: PathBuf,
    lock: Mutex<()>,
}

impl Log {
    pub fn new(path: PathBuf) -> Self {
        Log {
            path,
            lock: Mutex::new(()),
        }
    }

    /// Appends one timestamped line. Logging never fails the work it describes.
    pub fn line(&self, text: impl AsRef<str>) {
        let _held = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let text = text.as_ref().replace('\n', " | ");
        if std::fs::metadata(&self.path).is_ok_and(|m| m.len() as usize > MAX_BYTES) {
            if let Ok(old) = std::fs::read_to_string(&self.path) {
                let keep = old.lines().skip(old.lines().count() / 2).collect::<Vec<_>>().join("\n");
                let _ = std::fs::write(&self.path, keep + "\n");
            }
        }
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&self.path) {
            let _ = writeln!(f, "{} {text}", utc_now());
        }
    }

    /// Runs one step and records whether it worked and how long it took.
    pub async fn step<T>(&self, what: &str, work: impl Future<Output = Result<T>>) -> Result<T> {
        let started = Instant::now();
        let result = work.await;
        let secs = started.elapsed().as_secs_f64();
        match &result {
            Ok(_) => self.line(format!("{what}: ok in {secs:.1}s")),
            Err(e) => self.line(format!("{what}: FAILED after {secs:.1}s: {e}")),
        }
        result
    }

    /// The last [`REPORT_LINES`] lines.
    pub fn tail(&self) -> String {
        let _held = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let text = std::fs::read_to_string(&self.path).unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        lines[lines.len().saturating_sub(REPORT_LINES)..].join("\n")
    }
}

/// One line describing a video, e.g. "1080x1920 30.00fps h264 yuv420p 8-bit SDR, 42.1s, audio aac".
pub fn describe_media(m: &MediaInfo) -> String {
    format!(
        "{}x{} {:.2}fps {} {} {}-bit {}{}, {:.1}s, audio {}, {:.1} MB",
        m.width,
        m.height,
        m.fps,
        m.video_codec,
        m.pix_fmt,
        m.bit_depth,
        match m.hdr {
            crate::probe::Hdr::None => "SDR".to_string(),
            hdr => format!("HDR {hdr:?}"),
        },
        if m.dolby_vision { " Dolby Vision" } else { "" },
        m.duration,
        m.audio_codec.as_deref().unwrap_or("none"),
        m.size_bytes as f64 / 1e6,
    )
}

pub fn describe_report(r: &QualityReport) -> String {
    let vmaf = r
        .vmaf
        .as_ref()
        .map(|v| format!("VMAF {:.2} (min {:.2})", v.mean, v.min))
        .unwrap_or_else(|| "VMAF none".into());
    format!(
        "{vmaf}, ceiling {:?}, quality {:?}, matches: resolution {} fps {} frames {:?} audio {} hdr {}, notes {:?}",
        r.ceiling, r.quality, r.resolution_match, r.fps_match, r.frame_count_match, r.audio_match, r.hdr_match, r.notes
    )
}

/// "2026-10-02 09:41:07Z"
fn utc_now() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format_utc(secs)
}

fn format_utc(secs: u64) -> String {
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_are_utc_dates() {
        assert_eq!(format_utc(0), "1970-01-01 00:00:00Z");
        assert_eq!(format_utc(1_790_789_656), "2026-09-30 17:34:16Z");
        assert_eq!(format_utc(951_782_400), "2000-02-29 00:00:00Z");
    }

    #[test]
    fn log_keeps_recent_lines_and_flattens_newlines() {
        let dir = std::env::temp_dir().join(format!("subtitles-log-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = Log::new(dir.join("log.txt"));
        log.line("first");
        log.line("two\nlines");
        let tail = log.tail();
        assert!(tail.contains("first"));
        assert!(tail.ends_with("two | lines"));
        for i in 0..20_000 {
            log.line(format!("filler {i}"));
        }
        assert!(std::fs::metadata(dir.join("log.txt")).unwrap().len() as usize <= MAX_BYTES + 100);
        assert!(log.tail().ends_with("filler 19999"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
