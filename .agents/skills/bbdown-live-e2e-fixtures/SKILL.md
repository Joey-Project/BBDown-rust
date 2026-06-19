---
name: bbdown-live-e2e-fixtures
description: Use when BBDown-rust work needs real Bilibili live e2e URLs, fixture selection for normal videos, multi-page videos, restricted-area bangumi series, or restricted-area episode regression checks.
---

# BBDown Live E2E Fixtures

## Overview

Use this repo-local skill when validating BBDown-rust behavior against real Bilibili pages instead
of mock fixtures. It keeps a curated live URL reference so future feature PRs can reuse the same
normal-video and restricted-area samples.

## Workflow

1. Read `references/live-fixtures.md` before choosing live e2e targets.
2. Prefer the smallest fixture subset that covers the changed behavior.
3. Use mock e2e tests for deterministic CI coverage; use these URLs for opt-in live validation.
4. For restricted-area fixtures, use the repo's existing credentials/proxy/access-key setup and
   document which access path was exercised.
5. Do not paste credentials, proxy secrets, cookies, or access keys into logs, docs, PR comments, or
   review prompts.

## Reporting

When reporting live validation, include:

- fixture label;
- command or harness invoked;
- whether credentials/proxy were required;
- pass/fail/skip result;
- the reason for any skip.
