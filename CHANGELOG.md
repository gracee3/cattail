# Changelog

Notable changes to `cattail` are recorded here. Before 1.0, compatibility
decisions are made release by release and called out explicitly.

## Unreleased

- Correct the documented lifecycle after confirming the crates.io and GitHub
  v0.1.0 publications.
- Raise the development MSRV from the stale 1.74 declaration to Rust 1.85, the
  minimum required by the locked Clap 4.6 dependency line.
- Pin current CI to Rust 1.97.1, validate the MSRV separately, remove permissive
  package flags, and verify generated release assets.
- Update the follow buffer loop for the Rust 1.97 Clippy lint set without
  changing its line-buffering behavior.

## 0.1.0 — 2026-03-20

- Added multi-file and glob-based tailing with deterministic source labels.
- Added backlog and `--since-now` modes.
- Added notify-backed wakeups with polling recovery and dynamic file discovery.
- Defined truncation and delete/recreate behavior.
- Added stress coverage, a smoke demonstration, generated man pages and shell
  completions, crates.io packaging, and the initial GitHub release.
