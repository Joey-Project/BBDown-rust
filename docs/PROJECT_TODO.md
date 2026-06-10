# Project TODO

- [completed] Land the Rust workspace, CI, architecture docs, metadata resolver, credential store, and CLI `info/auth` foundation.
- [completed] Add stream resolver chain, download planning, subtitle discovery, and danmaku discovery.
- [completed] Add real file downloads, retry/resume policy, ffmpeg mux integration, and mock e2e download coverage.
- [completed] Add QR login flow and live-test opt-in harness.
- [completed] Add restricted-area proxy resolver ordering and diagnostics.
- [completed] Strengthen local live e2e coverage with a manifest-driven sample matrix.
- [completed] Add GitHub release binary packaging for tagged and manual builds.
- [completed] Add crate publish readiness and dry-run validation.
- [completed] Add clearer stream quality selection and listing support.
- [completed] Expand restricted-area proxy response compatibility.
- [completed] Harden integration APIs and embedding documentation.
- [completed] Add download archive and duplicate decision handling.
- [completed] Add more input parsing and batch collection parsing for B23 short links, cheese
  courses, favorite lists, space videos, collections, and series.
- [completed] Add bilingual human-facing docs and rerun real live e2e samples.
- [completed] Rename the publishable library package to `bbdown-core` before the 0.1.0 release.
- [completed] Add protected release candidate creation and RC promotion workflows for GitHub
  Release and crates.io publication.
- [pending] Add cover download support to the crate and CLI.
- [pending] Add single-download modes for video-only, audio-only, subtitle-only, danmaku-only, and
  cover-only workflows.
- [pending] Add ASS-format danmaku sidecar generation.
- [pending] Add UPOS host replacement controls and PCDN filtering/handling.
- [pending] Continue BBDown parity work after the current slices: richer filename templates,
  additional app/TV playurl modes, richer selection syntax, API/server integration surfaces,
  aria2 or multi-thread download integration, MP4Box muxing, and subtitle-to-SRT conversion.
