# Live E2E Fixtures

Use these URLs for opt-in live validation after BBDown-rust changes. They are not deterministic CI
fixtures; page availability, titles, codecs, region policy, and account requirements can change.

| Label | URL | Purpose | Access Notes |
| --- | --- | --- | --- |
| `normal-playlist-video` | `https://www.bilibili.com/video/BV1QtjA6BEB8/` | Normal public video with a playlist/list-style surface. Use for input parsing, page/list selection, planning, playback metadata, and ordinary download smoke checks. | Should not require restricted-area proxy. |
| `normal-multipage-video` | `https://www.bilibili.com/video/BV1uW4y1s7zN/` | Normal public video with multiple pages. Use for page selection, batch entry planning, file naming, and archive identity checks. | Should not require restricted-area proxy. |
| `restricted-bangumi-series` | `https://www.bilibili.com/bangumi/media/md28338980` | Restricted-area bangumi series page. Use for season/media parsing, selected episode planning, and restricted-area fallback coverage. | Requires the repo's configured restricted-area credentials/proxy path. |
| `restricted-bangumi-episode` | `https://www.bilibili.com/bangumi/play/ep664928` | Restricted-area bangumi episode page. Use for direct episode parsing, stream planning, download smoke checks, and restricted-area fallback coverage. | Requires the repo's configured restricted-area credentials/proxy path. |

## Selection Guidance

- Input parser changes: run both public video fixtures plus the restricted episode fixture.
- Page or episode selection changes: run `normal-multipage-video`, `restricted-bangumi-series`, and
  `restricted-bangumi-episode`.
- Restricted-area resolver changes: run both restricted fixtures and record the access path used.
- Download execution changes: prefer a narrow smoke run against one public fixture first, then one
  restricted fixture if the change touches PGC or proxy flow.
- Playback/source reporting changes: run at least one public video and one restricted fixture, then
  inspect selected media source URLs, backup URLs, headers, mime/codec, duration, and size fields.

## Evidence Notes

- Record absolute date/time for live runs because upstream behavior can change.
- Record skipped fixtures explicitly, especially when credentials, proxy, or access keys are not
  available in the current environment.
- Keep live logs redacted. Do not store credential values, cookies, signed URL tokens, or access
  keys in tracked files.
