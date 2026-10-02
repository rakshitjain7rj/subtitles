# Subtitles

A desktop app for short-form creators: it turns Hindi or Hinglish speech into
English captions burned into the video, without the resolution drop and heavy
re-compression that caption apps usually add. After every export it measures
the result against the original and shows the score.

## How it works

1. **Import** a video. The app reads its resolution, frame rate, HDR format
   and audio, and makes a small preview copy.
2. **Transcribe** with ElevenLabs Scribe v2, which returns every spoken word
   with its timing. Only a mono audio track is uploaded.
3. **Translate** with Gemini 3.8 Flash (free tier) or Claude Sonnet 5.5
   (paid), chosen in Settings. The whole transcript is sent as context and
   comes back as 2 to 5 word English phrases, each tied to the words it is
   spoken over.
4. **Review**: English next to the Hindi it came from, with a live preview.
   Edit text, nudge timing, split, merge, add or delete captions, and set the
   caption position and size.
5. **Export** once. The source resolution and frame timing are kept, the audio
   stream is copied untouched, SDR is encoded as H.264 and HDR stays HDR as
   10-bit HEVC. **High** quality (default, CRF 12) is visually identical at
   about the original's size; **Lossless** keeps every pixel outside the
   captions exact, at several times the size.
6. **Quality score**: VMAF against the original, measured on the part of the
   picture outside the captions, shown against the original's score against
   itself (the most that video can reach, often below 100), plus PSNR and a
   check that resolution, frame rate, shown frames, audio and dynamic range
   all match.

Each video is saved as a project, so transcription and translation are never
paid for twice.

## API keys and privacy

The app has no server. You add your own API keys in Settings: ElevenLabs for
transcription, plus Gemini or Anthropic for translation. They are stored in
the operating system's keychain and usage goes to your own accounts.

- Transcription: about $0.22 per hour of audio (ElevenLabs' free plan
  includes some each month).
- Translation with Gemini 3.8 Flash: free within Google's free-tier limits.
  On the free tier Google may use the transcript text to improve its
  products, and people at Google may read it.
- Translation with Claude Sonnet 5.5: roughly $0.04 per minute of video.

Your video's audio goes to ElevenLabs and the transcript text goes to the
translator you chose. Nothing else leaves your computer.

## Development

Requirements: Node with pnpm, Rust, the
[Tauri prerequisites](https://tauri.app/start/prerequisites/) for your
platform, and an `ffmpeg`/`ffprobe` on `PATH` built with libx264, libx265,
libass and libvmaf.

```sh
pnpm install
pnpm tauri dev
```

Tests:

```sh
pnpm test                                  # caption editing and line breaking
pnpm exec tsc --noEmit                     # type check
cd src-tauri && cargo test                 # backend unit tests
cd src-tauri && cargo test -- --ignored    # real exports through ffmpeg
```

The `--ignored` tests generate SDR and HDR clips, run the full export and
quality measurement, and check the captions are in the picture.

To use a specific ffmpeg, set `SUBTITLES_FFMPEG_DIR` to the folder holding
`ffmpeg` and `ffprobe`. `scripts/fetch-ffmpeg.sh linux-x64` downloads the same
build the Linux installer ships into `src-tauri/bin`.

## Installers

`.github/workflows/release.yml` builds unsigned installers for Linux, Windows
and macOS, each with ffmpeg bundled, and runs the export tests against that
bundled ffmpeg first. Push a tag such as `v0.1.0` to get a draft release.

Because the installers are unsigned, Windows and macOS show a warning the
first time the app is opened.

### Releasing an update

Installed apps check GitHub Releases on launch and offer to update
themselves. To ship a new version:

1. Bump the version in `package.json`, `src-tauri/Cargo.toml` and
   `src-tauri/tauri.conf.json` (they must match), and commit.
2. `git tag v0.1.1 && git push --tags`. The workflow builds every platform
   into a draft release.
3. Try the draft's installers, then publish the release. Apps only see an
   update once it is published.

Updates are signed. The private key is the repository secret
`TAURI_SIGNING_PRIVATE_KEY`; its public half is in `tauri.conf.json`. If the
key is lost, installed apps can't be updated and everyone has to reinstall,
so keep a backup of it. On Linux, only the AppImage updates itself; the
`.deb` has to be reinstalled.

## Testers

[`TESTING.md`](TESTING.md) is the guide to send people trying the app: download, install past the unsigned-app warnings, set up keys, caption a video and send feedback.

## Layout

```
src/                    React review screen
  lib/captions.ts       caption editing operations
  lib/wrap.ts           line breaking shared by preview and export
  lib/style.ts          caption look (mirrors src-tauri/src/ass.rs)
src-tauri/src/
  pipeline.rs           import, transcribe, translate, export
  transcribe/           speech-to-text behind a swappable trait (Scribe v2)
  translate/            shared prompt and chunking; gemini.rs and claude.rs
  settings.rs           app preferences (which translator)
  encode.rs             ffmpeg arguments for preview and export
  ass.rs                caption rendering script
  quality.rs            VMAF and the like-for-like report
  project.rs            projects on disk
  keys.rs               API keys in the OS keychain
```

## Licence

GPL-3.0-or-later. The app bundles GPL builds of FFmpeg; see `THIRD-PARTY.md`.
