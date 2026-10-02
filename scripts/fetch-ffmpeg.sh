#!/usr/bin/env bash
# Downloads the GPL ffmpeg/ffprobe build that gets bundled into an installer
# and puts it in src-tauri/bin/. Used by the release workflow; also handy for
# testing the bundled build locally:
#
#   scripts/fetch-ffmpeg.sh linux-x64
#   SUBTITLES_FFMPEG_DIR=src-tauri/bin pnpm tauri dev
#
# Sources: BtbN/FFmpeg-Builds (Linux, Windows) and ffmpeg.martin-riedl.de
# (macOS). Both are static GPL builds with libx264, libx265, libass, libvmaf
# and zimg. See THIRD-PARTY.md.
set -euo pipefail

platform="${1:?usage: fetch-ffmpeg.sh <linux-x64|windows-x64|macos-arm64|macos-x64> [--no-verify]}"
verify=true
[[ "${2:-}" == "--no-verify" ]] && verify=false

root="$(cd "$(dirname "$0")/.." && pwd)"
out="$root/src-tauri/bin"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$out"

btbn="https://github.com/BtbN/FFmpeg-Builds/releases/download/latest"
riedl="https://ffmpeg.martin-riedl.de/redirect/latest/macos"
exe=""

case "$platform" in
  linux-x64)
    curl -fsSL --retry 3 -o "$work/ffmpeg.tar.xz" "$btbn/ffmpeg-n9.0-latest-linux64-gpl-9.0.tar.xz"
    tar -xf "$work/ffmpeg.tar.xz" -C "$work"
    cp "$work"/ffmpeg-*/bin/ffmpeg "$work"/ffmpeg-*/bin/ffprobe "$out/"
    cp "$work"/ffmpeg-*/LICENSE.txt "$out/FFMPEG-LICENSE.txt"
    ;;
  windows-x64)
    exe=".exe"
    curl -fsSL --retry 3 -o "$work/ffmpeg.zip" "$btbn/ffmpeg-n9.0-latest-win64-gpl-9.0.zip"
    unzip -q "$work/ffmpeg.zip" -d "$work"
    cp "$work"/ffmpeg-*/bin/ffmpeg.exe "$work"/ffmpeg-*/bin/ffprobe.exe "$out/"
    cp "$work"/ffmpeg-*/LICENSE.txt "$out/FFMPEG-LICENSE.txt"
    ;;
  macos-arm64 | macos-x64)
    arch="arm64"
    triple="aarch64-apple-darwin"
    if [[ "$platform" == "macos-x64" ]]; then
      arch="amd64"
      triple="x86_64-apple-darwin"
    fi
    for tool in ffmpeg ffprobe; do
      curl -fsSL --retry 3 -o "$work/$tool.zip" "$riedl/$arch/release/$tool.zip"
      unzip -q -o "$work/$tool.zip" -d "$work"
      chmod +x "$work/$tool"
      cp "$work/$tool" "$out/$tool"
      # Tauri bundles sidecars by target triple and signs them with the app.
      cp "$work/$tool" "$out/$tool-$triple"
    done
    ;;
  *)
    echo "unknown platform: $platform" >&2
    exit 1
    ;;
esac

chmod +x "$out/ffmpeg$exe" "$out/ffprobe$exe" 2>/dev/null || true

if $verify; then
  # Fail the build rather than ship an ffmpeg that can't do the export.
  encoders="$("$out/ffmpeg$exe" -hide_banner -encoders 2>/dev/null)"
  filters="$("$out/ffmpeg$exe" -hide_banner -filters 2>/dev/null)"
  for encoder in libx264 libx265; do
    grep -q " $encoder " <<<"$encoders" || { echo "bundled ffmpeg lacks $encoder" >&2; exit 1; }
  done
  for filter in ass libvmaf zscale tonemap setparams; do
    grep -Eq "^ [A-Z.]+ +$filter +" <<<"$filters" || { echo "bundled ffmpeg lacks the $filter filter" >&2; exit 1; }
  done
  "$out/ffprobe$exe" -version >/dev/null
  echo "ffmpeg for $platform is ready in $out:"
  "$out/ffmpeg$exe" -hide_banner -version | head -n 1
fi
