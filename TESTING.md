# Trying Subtitles

Thanks for testing! Subtitles turns Hindi or Hinglish speech in your video
into English captions burned into the picture, without lowering the video's
quality. This is an early version, so things may break. When they do, please
tell us (see [Sending feedback](#sending-feedback)).

Setting it up takes about 15 minutes, once.

## 1. Download

Open the [latest release](https://github.com/rakshitjain7rj/subtitles/releases/latest)
and, under **Assets**, download the file for your computer:

| Your computer | File to download |
| --- | --- |
| Windows | `Subtitles_…_x64-setup.exe` |
| Mac with Apple chip (M1, M2, M3, M4…) | `Subtitles_…_aarch64.dmg` |
| Mac with Intel chip | `Subtitles_…_x64.dmg` |
| Linux | `Subtitles_…_amd64.AppImage` |

Not sure which Mac you have? Apple menu  → **About This Mac**. If it says
**Chip: Apple M…**, take `aarch64`. If it says **Processor: Intel**, take
`x64`.

The download is about 110 MB, because the video tools come built in.

## 2. Install

The app isn't yet registered with Microsoft or Apple, so both show a warning
the first time. That's expected. Here's how to get past it.

### Windows

1. Open the downloaded `…-setup.exe`.
2. If a blue **Windows protected your PC** box appears, click
   **More info**, then **Run anyway**.
3. Follow the installer. Subtitles then appears in the Start menu.

### Mac

1. Open the downloaded `.dmg` and drag **Subtitles** into **Applications**.
2. Open **Subtitles** from Applications. The Mac will say it can't check the
   app for malicious software. Click **Done** (or **OK**).
3. Open **System Settings → Privacy & Security**, scroll down to
   *"Subtitles" was blocked…*, and click **Open Anyway**. Enter your Mac
   password if asked.
4. Open Subtitles again and click **Open Anyway** once more. After this it
   opens normally.

### Linux

Make the AppImage executable (`chmod +x Subtitles_*.AppImage`) and run it.

## 3. Connect your free accounts

The first time it opens, the app walks you through making two free API
keys. An API key is a password that lets the app use a service on your
account:

- **ElevenLabs** turns the speech into text. The free plan includes some
  transcription every month. Past that it costs about ₹19 per hour of audio,
  charged to your ElevenLabs account. Nothing is charged unless you add
  payment details there.
- **Gemini** (Google) translates it into English for free.

Each step has a button that opens the right page, plus instructions. Paste
each key into the app; it checks the key before saving it. Keys stay on your
computer.

## 4. Caption a video

1. Click **Open a video**, or drop one onto the window.
2. Click **Generate captions**. A one-minute clip takes about a minute.
3. Check the captions. The English is shown next to the Hindi it came from.
   Click a caption to fix its words, nudge its start and end, or use
   **Split**, **Merge ↓** and **Delete**. **Position** and **Size**, next to
   the preview, move and resize every caption.
4. Click **Export video**. When it's done, the app shows a quality score
   that compares the result with your original.

Each video is saved, so you can come back to it without paying to transcribe
it again.

## Updates

When there's a new version, the app offers it when you open it. Click
**Update and restart**.

## Sending feedback

Message us on WhatsApp. The most useful things to hear:

- Were the captions right? Copy a few that were wrong and what they should
  have said.
- Did the timing match the speech?
- Did the exported video look and sound exactly like your original?
- Anything confusing, slow or broken.

If something fails, open **Settings → Copy log** and paste the result into
your message. It has your computer and video details and any error
messages, but no API keys, transcripts or captions.

## Privacy

There is no Subtitles server. Your video's audio goes to ElevenLabs, and the
transcript text goes to Google Gemini, using your own accounts. Nothing else
leaves your computer. On Gemini's free tier, Google may use the text to
improve its products.
