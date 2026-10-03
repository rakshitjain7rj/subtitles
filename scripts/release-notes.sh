#!/usr/bin/env bash
# Prints the release description: which download is for which computer.
# Usage: scripts/release-notes.sh 0.1.2
set -euo pipefail

v="${1:?usage: release-notes.sh VERSION}"
base="https://github.com/rakshitjain7rj/subtitles/releases/download/v${v}"

cat <<EOF
## Download for your computer

| Your computer | Download |
| --- | --- |
| 🪟 **Windows** | [**Subtitles_${v}_x64-setup.exe**](${base}/Subtitles_${v}_x64-setup.exe) |
| 🍎 **Mac with Apple chip** (M1, M2, M3, M4…) | [**Subtitles_${v}_aarch64.dmg**](${base}/Subtitles_${v}_aarch64.dmg) |
| 🍎 **Mac with Intel chip** | [**Subtitles_${v}_x64.dmg**](${base}/Subtitles_${v}_x64.dmg) |
| 🐧 Linux (any) | [Subtitles_${v}_amd64.AppImage](${base}/Subtitles_${v}_amd64.AppImage) |
| 🐧 Linux (Ubuntu, Debian) | [Subtitles_${v}_amd64.deb](${base}/Subtitles_${v}_amd64.deb) |
| 🐧 Linux (Fedora, openSUSE) | [Subtitles-${v}-1.x86_64.rpm](${base}/Subtitles-${v}-1.x86_64.rpm) |

**Which Mac do I have?** Apple menu  → **About This Mac**. "Chip: Apple M…" means Apple chip; "Processor: Intel" means Intel.

**First time?** Follow the [step-by-step guide](https://github.com/rakshitjain7rj/subtitles/blob/main/TESTING.md). Windows and Mac show a warning the first time you open the app; the guide shows how to get past it.

**Already installed?** You don't need to download anything: the app offers this update itself when you open it.

<details>
<summary>What are the other files?</summary>

- \`Subtitles_${v}_x64_en-US.msi\`: an alternative Windows installer for company-managed PCs. Most people should use the \`.exe\`.
- \`.sig\`, \`.app.tar.gz\` and \`latest.json\`: used by the app's automatic updates. You don't need them.

</details>
EOF
