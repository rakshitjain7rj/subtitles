# Third-party software

Subtitles is licensed under the GNU General Public License, version 3 or later
(see `LICENSE`). Installers also contain the following.

## FFmpeg

Each installer bundles `ffmpeg` and `ffprobe`, used for all media work
(reading the video, encoding the export, rendering captions with libass, and
scoring quality with libvmaf). These are GPL builds that include libx264 and
libx265, which is why the app as a whole is distributed under the GPL.

| Platform | Build | Build scripts and source |
|---|---|---|
| Linux, Windows | BtbN FFmpeg-Builds, `n9.0` GPL static | https://github.com/BtbN/FFmpeg-Builds |
| macOS | Martin Riedl's static builds | https://ffmpeg.martin-riedl.de and https://git.martin-riedl.de/ffmpeg/build-script |

FFmpeg's own source is at https://ffmpeg.org/download.html. The binaries are
downloaded unmodified by `scripts/fetch-ffmpeg.sh` when an installer is built;
the licence text that ships with the Linux and Windows builds is included in
the installer as `bin/FFMPEG-LICENSE.txt`.

## Caption font

Montserrat ExtraBold, copyright The Montserrat Project Authors
(https://github.com/JulietaUla/Montserrat), under the SIL Open Font License 1.1.
The licence is in `src-tauri/resources/fonts/OFL.txt`.

## Libraries

The app is built with Tauri, React and the Rust and JavaScript packages listed
in `src-tauri/Cargo.toml` and `package.json`, each under its own
(MIT, Apache-2.0 or similarly permissive) licence.
