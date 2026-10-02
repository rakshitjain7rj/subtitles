//! The steps a video goes through: import, transcribe, translate, export.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::ass;
use crate::captions::{build_captions, with_line_breaks};
use crate::encode::Quality;
use crate::encode::{self, ASS_FILE, FONTS_DIR};
use crate::error::{msg, Result};
use crate::ffmpeg::{run_capture, run_with_progress, Tools};
use crate::keys::{self, Provider};
use crate::probe::{probe, MediaInfo};
use crate::project::{now, ExportRecord, Project, Store, AUDIO_FILE, PREVIEW_FILE};
use crate::quality::{self, Measurement, CEILING_LOG, VMAF_LOG};
use crate::transcribe::scribe::Scribe;
use crate::transcribe::Transcriber;
use crate::translate::{self, claude::Claude, gemini::Gemini, Engine};

const WORK_DIR: &str = "work";

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    Preview,
    Audio,
    Transcribe,
    Translate,
    Encode,
    Verify,
}

/// Reports `(stage, fraction)`; the fraction is `None` when a step can't
/// measure its own progress (a network call).
pub type Progress<'a> = &'a (dyn Fn(Stage, Option<f64>) + Send + Sync);

fn source_of(project: &Project) -> Result<PathBuf> {
    let path = PathBuf::from(&project.source_path);
    if !path.is_file() {
        return Err(msg(format!(
            "The original video is no longer at {}. Move it back to continue.",
            project.source_path
        )));
    }
    Ok(path)
}

/// Probes the video and builds its preview. Importing the same file again
/// reopens the existing project instead of starting over.
pub async fn import(tools: &Tools, store: &Store, source: &Path, progress: Progress<'_>) -> Result<Project> {
    if !source.is_file() {
        return Err(msg("That file could not be found."));
    }
    let source_str = source.to_string_lossy();
    if let Some(existing) = store.list().into_iter().find(|p| p.source_path == source_str) {
        return store.load(&existing.id);
    }

    let info = probe(tools, source).await?;
    let mut project = Project::new(source, info);
    store.save(&mut project)?;
    if let Err(e) = build_preview(tools, store, &project, progress).await {
        let _ = store.delete(&project.id);
        return Err(e);
    }
    Ok(project)
}

async fn build_preview(tools: &Tools, store: &Store, project: &Project, progress: Progress<'_>) -> Result<()> {
    let source = source_of(project)?;
    let out = store.dir(&project.id)?.join(PREVIEW_FILE);
    let report = |f: f64| progress(Stage::Preview, Some(f));
    let duration = project.info.duration;
    let with_tonemap = encode::preview_args(&project.info, &source, &out, true);
    match run_with_progress(&tools.ffmpeg, &with_tonemap, None, duration, report).await {
        Ok(()) => Ok(()),
        // An ffmpeg built without zscale can't tone-map; a flat-looking
        // preview is better than none. The export is unaffected.
        Err(_) if project.info.hdr != crate::probe::Hdr::None => {
            let plain = encode::preview_args(&project.info, &source, &out, false);
            run_with_progress(&tools.ffmpeg, &plain, None, duration, report).await
        }
        Err(e) => Err(e),
    }
}

/// Extracts the audio and transcribes it. A project that already has a
/// transcript is returned untouched unless `force` is set, so the paid call
/// is never repeated by accident.
pub async fn transcribe(
    tools: &Tools,
    store: &Store,
    id: &str,
    force: bool,
    progress: Progress<'_>,
) -> Result<Project> {
    let project = store.load(id)?;
    if project.words.is_some() && !force {
        return Ok(project);
    }
    let api_key = keys::require(Provider::ElevenLabs).await?;
    let source = source_of(&project)?;
    if project.info.audio_codec.is_none() {
        return Err(msg("This video has no audio track to transcribe."));
    }

    let audio = store.dir(id)?.join(AUDIO_FILE);
    if !audio.is_file() {
        let partial = audio.with_extension("partial.flac");
        run_with_progress(
            &tools.ffmpeg,
            &encode::audio_args(&source, &partial),
            None,
            project.info.duration,
            |f| progress(Stage::Audio, Some(f)),
        )
        .await?;
        tokio::fs::rename(&partial, &audio).await?;
    }

    progress(Stage::Transcribe, None);
    let transcript = Scribe::new(api_key).transcribe(&audio).await?;

    // Reload: the user may have kept editing while the request was in flight.
    let mut project = store.load(id)?;
    project.words = Some(transcript.words);
    project.language = transcript.language;
    project.translated = false;
    project.translated_with = None;
    project.captions.clear();
    store.save(&mut project)?;
    Ok(project)
}

/// Translates the transcript into captions, replacing any existing ones.
pub async fn translate(store: &Store, id: &str, engine: Engine, progress: Progress<'_>) -> Result<Project> {
    let project = store.load(id)?;
    let words = project
        .words
        .clone()
        .ok_or_else(|| msg("Transcribe the video before translating it."))?;
    let report = |f| progress(Stage::Translate, Some(f));
    progress(Stage::Translate, None);
    let translation = match engine {
        Engine::Gemini => {
            let key = keys::require(Provider::Gemini).await?;
            translate::translate(&Gemini::new(key), &words, report).await?
        }
        Engine::Claude => {
            let key = keys::require(Provider::Anthropic).await?;
            translate::translate(&Claude::new(key), &words, report).await?
        }
    };
    let captions = build_captions(&words, &translation.phrases);
    if captions.is_empty() {
        return Err(msg("The translation produced no captions."));
    }

    let mut project = store.load(id)?;
    project.captions = captions;
    project.translated = true;
    project.translated_with = Some(translation.model_label());
    store.save(&mut project)?;
    Ok(project)
}

/// Where an export of this project would be saved by default.
pub fn default_export_path(project: &Project) -> PathBuf {
    let source = Path::new(&project.source_path);
    let ext = encode::output_extension(source, &project.info);
    let stem = source.file_stem().and_then(|s| s.to_str()).unwrap_or("video");
    source.with_file_name(format!("{stem}.captioned.{ext}"))
}

/// Burns the captions in, then measures the result against the source.
/// `wrapped` carries the review screen's line breaks, one entry per caption,
/// so the export wraps exactly as the preview did; without it (or if it
/// doesn't match the saved captions) libass wraps long captions itself.
pub async fn export(
    tools: &Tools,
    store: &Store,
    id: &str,
    out: &Path,
    wrapped: &[String],
    quality: Quality,
    progress: Progress<'_>,
) -> Result<Project> {
    let mut project = store.load(id)?;
    let source = source_of(&project)?;
    if project.captions.iter().all(|c| c.english.trim().is_empty()) {
        return Err(msg("There are no captions to export yet."));
    }
    if same_file(&source, out) {
        return Err(msg(
            "Choose a different name: the export can't overwrite the original video.",
        ));
    }
    // Re-read the source: projects made by older versions may hold stale
    // details (such as a frame count that included hidden frames).
    project.info = probe(tools, &source).await?;
    let info = &project.info;

    let work = store.dir(id)?.join(WORK_DIR);
    let fonts = work.join(FONTS_DIR);
    tokio::fs::create_dir_all(&fonts).await?;
    tokio::fs::write(fonts.join(ass::FONT_FILE), ass::FONT_BYTES).await?;
    let broken = with_line_breaks(&project.captions, wrapped);
    let pre_wrapped = broken.is_some();
    let captions = broken.unwrap_or_else(|| project.captions.clone());
    let layout = ass::layout(info.width, info.height, project.style, &captions, pre_wrapped);
    let script = ass::render(info.width, info.height, &layout, &captions, info.hdr, pre_wrapped);
    tokio::fs::write(work.join(ASS_FILE), script).await?;

    // Encode next to the destination under a temporary name, so a failed or
    // interrupted export never leaves a broken file with the final name.
    let ext = out.extension().and_then(|e| e.to_str()).unwrap_or("mp4");
    let partial = out.with_extension(format!("partial.{ext}"));
    let args = encode::export_args(info, &source, &partial, quality);
    let encoded = run_with_progress(&tools.ffmpeg, &args, Some(&work), info.duration, |f| {
        progress(Stage::Encode, Some(f))
    })
    .await;
    if let Err(e) = encoded {
        let _ = tokio::fs::remove_file(&partial).await;
        return Err(e);
    }
    tokio::fs::rename(&partial, out).await?;

    progress(Stage::Verify, Some(0.0));
    let export_info = probe(tools, out).await?;
    let m = measure(tools, &work, &source, out, info, &layout, progress).await;
    let report = quality::build_report(info.clone(), export_info, quality, m);

    let fresh_info = project.info.clone();
    let mut project = store.load(id)?;
    project.info = fresh_info;
    project.last_export = Some(ExportRecord {
        path: out.to_string_lossy().into_owned(),
        at: now(),
        report,
    });
    store.save(&mut project)?;
    Ok(project)
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Runs VMAF, plus the source against itself for the ceiling. A failure here
/// doesn't fail the export (the file is already written); it is reported as
/// a note instead of a score.
async fn measure(
    tools: &Tools,
    work: &Path,
    source: &Path,
    export: &Path,
    info: &MediaInfo,
    layout: &ass::Layout,
    progress: Progress<'_>,
) -> Measurement {
    let bands = quality::measured_bands(info.height, layout.band_top, layout.band_bottom);
    let step = quality::frame_step(info.frame_count);
    let mut m = Measurement {
        vmaf: None,
        ceiling: None,
        fraction: 0.0,
        step,
        notes: Vec::new(),
    };
    if bands.is_empty() {
        m.notes
            .push("The captions cover too much of the frame to measure quality around them.".into());
        return m;
    }
    m.fraction = bands.iter().map(|(_, h)| *h as f64).sum::<f64>() / info.height as f64;
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .min(4);
    let args = quality::vmaf_args(source, export, info, &bands, step, threads);
    for log in [VMAF_LOG, CEILING_LOG] {
        let _ = tokio::fs::remove_file(work.join(log)).await;
    }

    let run = run_with_progress(&tools.ffmpeg, &args, Some(work), info.duration, |f| {
        progress(Stage::Verify, Some(f))
    })
    .await;
    let read = |name: &'static str| async move {
        let log = tokio::fs::read_to_string(work.join(name)).await?;
        quality::parse_vmaf_log(&log)
    };
    match run {
        Ok(()) => match read(VMAF_LOG).await {
            Ok(stats) => {
                m.vmaf = Some(stats);
                m.ceiling = read(CEILING_LOG).await.ok().map(|c| c.mean);
            }
            Err(e) => m.notes.push(format!("The quality score could not be measured: {e}")),
        },
        Err(e) => m.notes.push(format!("The quality score could not be measured: {e}")),
    }
    m
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolCheck {
    pub ok: bool,
    pub version: Option<String>,
    /// Human-readable list of what this ffmpeg build lacks.
    pub missing: Vec<String>,
}

/// Confirms ffmpeg is present and has everything the export needs.
pub async fn check_tools(tools: &Tools) -> ToolCheck {
    let arg = |s: &str| vec!["-hide_banner".to_string(), s.to_string()];
    let Ok(version) = run_capture(&tools.ffmpeg, &arg("-version"), None).await else {
        return ToolCheck {
            ok: false,
            version: None,
            missing: vec!["ffmpeg was not found".into()],
        };
    };
    let encoders = run_capture(&tools.ffmpeg, &arg("-encoders"), None)
        .await
        .unwrap_or_default();
    let filters = run_capture(&tools.ffmpeg, &arg("-filters"), None)
        .await
        .unwrap_or_default();
    let has_filter = |name: &str| filters.lines().any(|l| l.split_whitespace().nth(1) == Some(name));

    let mut missing = Vec::new();
    for (present, what) in [
        (encoders.contains("libx264"), "the H.264 encoder (libx264)"),
        (encoders.contains("libx265"), "the HEVC encoder (libx265)"),
        (has_filter("ass"), "caption rendering (libass)"),
        (has_filter("libvmaf"), "quality scoring (libvmaf)"),
    ] {
        if !present {
            missing.push(what.to_string());
        }
    }
    if run_capture(&tools.ffprobe, &arg("-version"), None).await.is_err() {
        missing.push("ffprobe".into());
    }
    let version = version
        .lines()
        .next()
        .map(|l| l.trim_start_matches("ffmpeg version ").to_string());
    ToolCheck {
        ok: missing.is_empty(),
        version,
        missing,
    }
}

/// End-to-end checks against a real ffmpeg. Run with
/// `cargo test -- --ignored`; they are skipped by default because they need
/// ffmpeg on PATH and take a while.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::captions::Caption;
    use crate::probe::Hdr;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    fn caption(start: f64, end: f64, english: &str) -> Caption {
        Caption {
            id: uuid::Uuid::new_v4().to_string(),
            start,
            end,
            english: english.into(),
            hindi: String::new(),
        }
    }

    async fn round_trip(source_args: &[&str], file: &str, quality: Quality) -> (Project, PathBuf) {
        let tools = Tools::locate(None);
        let root = std::env::temp_dir().join(format!("subtitles-e2e-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join(file);
        let mut args = strings(source_args);
        args.push(source.to_string_lossy().into_owned());
        run_capture(&tools.ffmpeg, &args, None).await.expect("create test clip");

        let store = Store::new(root.join("projects"));
        let quiet = |_: Stage, _: Option<f64>| {};
        let mut project = import(&tools, &store, &source, &quiet).await.expect("import");
        assert!(store.dir(&project.id).unwrap().join(PREVIEW_FILE).is_file());
        // Importing the same file again reopens the project.
        assert_eq!(import(&tools, &store, &source, &quiet).await.unwrap().id, project.id);

        project.captions = vec![
            caption(0.2, 1.4, "This is a test"),
            caption(1.4, 2.6, "of burned-in captions"),
        ];
        store.save(&mut project).unwrap();
        let out = default_export_path(&project);
        let wrapped = vec!["This is\na test".to_string(), "of burned-in captions".to_string()];
        let project = export(&tools, &store, &project.id, &out, &wrapped, quality, &quiet)
            .await
            .expect("export");
        assert!(out.is_file());
        assert!(!out.with_extension("partial.mp4").exists());
        (project, root)
    }

    #[tokio::test]
    #[ignore]
    async fn sdr_export_matches_the_source() {
        let (project, root) = round_trip(
            &[
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=720x1280:rate=30:duration=3",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=3",
                "-c:v",
                "libx264",
                "-crf",
                "18",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-shortest",
            ],
            "clip.mp4",
            Quality::High,
        )
        .await;
        let report = project.last_export.clone().unwrap().report;
        assert!(report.resolution_match && report.fps_match && report.audio_match && report.hdr_match);
        assert_eq!(report.frame_count_match, Some(true));
        assert_eq!(report.export.video_codec, "h264");
        let vmaf = report.vmaf.expect("vmaf score");
        assert!(vmaf.mean > 93.0, "vmaf mean {}", vmaf.mean);
        let ceiling = report.ceiling.expect("ceiling");
        assert!(
            vmaf.mean <= ceiling + 0.01 && ceiling - vmaf.mean < 1.0,
            "{} vs {ceiling}",
            vmaf.mean
        );
        assert!(vmaf.psnr_y.is_some_and(|p| p > 40.0));
        assert!(report.measured_fraction > 0.5);

        // The captions really are in the picture: inside the caption band the
        // export differs sharply from the source while a caption is up, and
        // matches it once the last caption has gone.
        let export_path = default_export_path(&project);
        let graph = "[0:v]crop=720:140:0:852[a];[1:v]crop=720:140:0:852[b];[a][b]psnr=stats_file=psnr.log";
        let mut args = strings(&["-v", "error", "-i"]);
        args.push(export_path.to_string_lossy().into_owned());
        args.push("-i".into());
        args.push(project.source_path.clone());
        args.extend(strings(&["-lavfi", graph, "-f", "null", "-"]));
        run_capture(&Tools::locate(None).ffmpeg, &args, Some(&root))
            .await
            .expect("psnr");
        let psnr: Vec<f64> = std::fs::read_to_string(root.join("psnr.log"))
            .unwrap()
            .lines()
            .filter_map(|l| l.split_whitespace().find_map(|f| f.strip_prefix("psnr_avg:")))
            .map(|v| v.parse().unwrap_or(f64::INFINITY))
            .collect();
        assert_eq!(psnr.len(), 90);
        assert!(psnr[24] < 25.0, "caption missing at 0.8s: band psnr {}", psnr[24]);
        assert!(psnr[60] < 25.0, "caption missing at 2.0s: band psnr {}", psnr[60]);
        assert!(
            psnr[85] > 35.0,
            "band should match the source after the captions: {}",
            psnr[85]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn lossless_export_scores_the_ceiling() {
        let (project, root) = round_trip(
            &[
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=640x360:rate=30:duration=2",
                "-c:v",
                "libx264",
                "-crf",
                "20",
                "-pix_fmt",
                "yuv420p",
            ],
            "lossless.mp4",
            Quality::Lossless,
        )
        .await;
        let report = project.last_export.unwrap().report;
        assert_eq!(report.quality, Quality::Lossless);
        let vmaf = report.vmaf.expect("vmaf");
        assert!(
            (report.ceiling.unwrap() - vmaf.mean).abs() < 0.001,
            "{vmaf:?} vs {:?}",
            report.ceiling
        );
        assert!(vmaf.psnr_y.unwrap() >= 59.9);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn hdr_export_stays_hdr() {
        let (project, root) = round_trip(
            &[
                "-v", "error", "-f", "lavfi", "-i", "testsrc2=size=640x360:rate=25:duration=2", "-vf",
                "format=yuv420p10le,setparams=color_primaries=bt2020:color_trc=smpte2084:colorspace=bt2020nc:range=tv",
                "-c:v", "libx265", "-x265-params",
                "log-level=error:hdr10-opt=1:repeat-headers=1:master-display=G(13250,34500)B(7500,3000)R(34000,16000)WP(15635,16450)L(10000000,50):max-cll=1000,400",
                "-tag:v", "hvc1",
            ],
            "hdr.mp4",
            Quality::High,
        )
        .await;
        assert_eq!(project.info.hdr, Hdr::Pq);
        assert_eq!(project.info.max_cll.as_deref(), Some("1000,400"));
        assert!(project
            .info
            .master_display
            .as_deref()
            .is_some_and(|m| m.ends_with("L(10000000,50)")));
        let report = project.last_export.unwrap().report;
        assert_eq!(report.export.hdr, Hdr::Pq);
        assert_eq!(report.export.bit_depth, 10);
        assert_eq!(report.export.video_codec, "hevc");
        assert_eq!(report.export.color_primaries.as_deref(), Some("bt2020"));
        assert_eq!(report.export.max_cll.as_deref(), Some("1000,400"));
        assert_eq!(report.export.master_display, project.info.master_display);
        assert!(report.hdr_match && report.resolution_match && report.fps_match);
        assert!(report.vmaf.is_some(), "notes: {:?}", report.notes);
        std::fs::remove_dir_all(root).unwrap();
    }
}
