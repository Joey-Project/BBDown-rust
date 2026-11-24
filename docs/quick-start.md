[ English | [简体中文](quick-start.zh-CN.md) ]

# BBDown CLI Quick Start

## 1. Download and run

Download and extract the archive for your OS and CPU architecture from [GitHub Releases](https://github.com/Joey-Project/BBDown-rust/releases/latest). Run `bbdown` from `PATH`. From the extracted folder, use `./bbdown` on macOS and Linux or `.\bbdown.exe` in Windows PowerShell.

<details>
<summary>FFmpeg setup</summary>

BBDown uses FFmpeg to create an MP4 by default. [Download FFmpeg](https://ffmpeg.org/download.html), then make it available on `PATH` or pass its path with `--ffmpeg`, for example `--ffmpeg ./tools/ffmpeg` or `--ffmpeg "C:\ffmpeg\bin\ffmpeg.exe"`. On Windows, you can put `ffmpeg.exe` next to `bbdown.exe` and run BBDown from that folder. Use `--no-mux` to keep the downloaded media files without muxing.

</details>

## 2. Optional web login

For membership access or higher quality, run this command.

```bash
bbdown auth login-web
```

The published v0.11.2 release prints a `scan:` URL. Convert it to a QR code with a local tool. Builds with automatic QR rendering show the code directly when the terminal has enough room. To save an SVG when output is redirected or the terminal is too small, run `bbdown auth login-web --qr-output login.svg`. Scan the code with the Bilibili app. BBDown saves the login after confirmation.

## 3. Download a video or episode

Replace the example with your Bilibili link or ID. Add `--output-dir downloads` to save files under `downloads`.

| What to download | Command |
| --- | --- |
| Regular video (UGC) | `bbdown download BV1qt4y1X7TW` or `bbdown download av170001` |
| Anime, film, or documentary episode (PGC) | `bbdown download ep318903` |
| Full PGC season | `bbdown download ss26801 --select all` |
| Selected episodes | `bbdown download ss26801 --select 1,3-5` |

For a media page (`md...`), `--select` picks episodes within the resolved season. `--select all` may include extras such as trailers. Use `bbdown info ss26801 --select all` to see the episode list.

## 4. Video quality, subtitles, and danmaku

- Run `bbdown plan av170001` to see selectable quality IDs under `videos`, then add `--video-quality <ID>` to the download command.
- Available subtitles, including AI tracks, are saved as separate files by default. Use `--only subtitle` to download only subtitles or `--no-subtitles` to skip them. The plan marks recognized AI tracks with `ai=true`. Use `--subtitle-ai exclude-ai` to skip AI tracks or `--subtitle-ai prefer-non-ai` to prefer human subtitles in the same language.
- XML danmaku is saved by default. Use `--danmaku-format xml,ass` for both formats, `--only danmaku` to download only danmaku, or `--no-danmaku` to skip it.

See the [User Guide](user-guide.md) for more options.
