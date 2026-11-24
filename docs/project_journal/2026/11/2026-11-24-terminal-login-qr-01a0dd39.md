---
id: 20261124-terminal-login-qr-01a0dd39
title: Terminal QR codes for CLI login
status: completed
created: 2026-11-24
updated: 2026-11-24
branch: codex/terminal-login-qr
pr:
supersedes: []
superseded_by:
---

# Terminal QR codes for CLI login

## Summary

- Render the Web and TV login tickets as QR codes in the terminal. Preserve the login URL for
  redirected output and give an actionable message when the QR code will not fit.
- Use terminal foreground and background colors when `NO_COLOR` is set. `--qr-output FILE` writes a
  standalone SVG to a new file, including when stdout is redirected or JSON mode is enabled.
- Keep QR presentation in the CLI. The core ticket, polling, and credential storage contracts stay
  the same.

## Validation

- The four renderer tests cover terminal size, plain glyph polarity, SVG output, and file protection.
  The existing Web and TV login tests exercise the JSON event and credential storage contracts.
- A bounded pseudo-terminal test with a loopback login server confirmed color and plain QR output,
  the size hint, SVG export with redirected and JSON output, and protection for an existing file.
- `just ci` passed formatting, strict Clippy, the Rust 1.95 workspace check, workspace tests,
  standalone CLI end-to-end tests, and the core crate publish dry run. The workspace suite passed
  1,073 tests with four ignored. The 215 standalone CLI tests overlap workspace coverage.
- The QR module also passed a standalone compiler check for `x86_64-pc-windows-gnu`.
