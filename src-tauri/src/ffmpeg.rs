//! Locating and running the ffmpeg/ffprobe binaries.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::error::{msg, Result};

const STDERR_TAIL_LINES: usize = 25;

#[derive(Debug, Clone)]
pub struct Tools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

impl Tools {
    /// Prefers, in order: `SUBTITLES_FFMPEG_DIR`, the binaries bundled with the
    /// installer, binaries next to the executable, then whatever is on PATH.
    pub fn locate(resource_dir: Option<&Path>) -> Tools {
        let mut dirs: Vec<PathBuf> = Vec::new();
        if let Some(dir) = std::env::var_os("SUBTITLES_FFMPEG_DIR") {
            dirs.push(PathBuf::from(dir));
        }
        if let Some(res) = resource_dir {
            dirs.push(res.join("bin"));
        }
        if let Some(dir) = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
        {
            dirs.push(dir);
        }
        let find = |name: &str| {
            dirs.iter()
                .map(|d| d.join(exe(name)))
                .find(|p| p.is_file())
                .unwrap_or_else(|| PathBuf::from(exe(name)))
        };
        Tools {
            ffmpeg: find("ffmpeg"),
            ffprobe: find("ffprobe"),
        }
    }
}

fn command(bin: &Path) -> Command {
    let mut cmd = Command::new(bin);
    cmd.stdin(Stdio::null()).kill_on_drop(true);
    // CREATE_NO_WINDOW: don't flash a console for every ffmpeg call.
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    cmd
}

fn spawn_error(bin: &Path, e: std::io::Error) -> crate::error::AppError {
    msg(format!("Could not start {}: {e}", bin.display()))
}

/// Runs to completion and returns stdout.
pub async fn run_capture(bin: &Path, args: &[String], cwd: Option<&Path>) -> Result<String> {
    let mut cmd = command(bin);
    cmd.args(args);
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    let out = cmd.output().await.map_err(|e| spawn_error(bin, e))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let tail: Vec<&str> = stderr.lines().rev().take(STDERR_TAIL_LINES).collect();
        let tail: Vec<&str> = tail.into_iter().rev().collect();
        return Err(msg(format!("{} failed:\n{}", bin.display(), tail.join("\n"))));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Parses one line of `-progress` output into seconds of output written.
pub fn parse_progress_line(line: &str) -> Option<f64> {
    let us = line.strip_prefix("out_time_us=")?.trim().parse::<i64>().ok()?;
    (us >= 0).then(|| us as f64 / 1_000_000.0)
}

/// Runs ffmpeg, reporting progress as a 0..1 fraction of `duration` seconds.
/// `-progress pipe:1 -nostats` is added to the arguments.
pub async fn run_with_progress(
    bin: &Path,
    args: &[String],
    cwd: Option<&Path>,
    duration: f64,
    mut on_progress: impl FnMut(f64),
) -> Result<()> {
    let mut cmd = command(bin);
    cmd.args(["-hide_banner", "-nostats", "-progress", "pipe:1"]).args(args);
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| spawn_error(bin, e))?;

    // stderr must be drained while we read stdout, or a chatty ffmpeg blocks.
    let stderr = child.stderr.take().expect("stderr piped");
    let tail_task = tokio::spawn(async move {
        let mut tail: VecDeque<String> = VecDeque::new();
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if tail.len() == STDERR_TAIL_LINES {
                tail.pop_front();
            }
            tail.push_back(line);
        }
        tail
    });

    let stdout = child.stdout.take().expect("stdout piped");
    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if let Some(secs) = parse_progress_line(&line) {
            if duration > 0.0 {
                on_progress((secs / duration).clamp(0.0, 1.0));
            }
        }
    }

    let status = child.wait().await?;
    let tail = tail_task.await.unwrap_or_default();
    if !status.success() {
        let tail: Vec<String> = tail.into_iter().collect();
        return Err(msg(format!("ffmpeg failed:\n{}", tail.join("\n"))));
    }
    on_progress(1.0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_lines() {
        assert_eq!(parse_progress_line("out_time_us=1500000"), Some(1.5));
        assert_eq!(parse_progress_line("out_time_us=N/A"), None);
        assert_eq!(parse_progress_line("out_time_us=-9223372036854775807"), None);
        assert_eq!(parse_progress_line("frame=12"), None);
    }
}
