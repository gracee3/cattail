# cattail

**Status:** Released. Version 0.1.0 is published on
[crates.io](https://crates.io/crates/cattail) and as a matching
[GitHub release](https://github.com/gracee3/cattail/releases/tag/v0.1.0).
Current `main` contains post-release maintenance work; see the
[changelog](CHANGELOG.md).

[![CI](https://github.com/gracee3/cattail/actions/workflows/ci.yml/badge.svg)](https://github.com/gracee3/cattail/actions/workflows/ci.yml)
[![license: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

`cattail` tails multiple files and glob patterns at once, keeps a prefixed line
per record, and follows new data as files grow or reappear.

It is designed for log-style files in active use:

- startup glob expansion and deduped resolution
- initial backlog display with `-n, --lines`
- explicit `--since-now` for "start at the end" behavior
- concurrent per-file follow workers
- truncation and delete/recreate recovery
- notify-backed watching with polling fallback
- dynamic discovery of files created later that match startup globs

## Install

Install the released crate and its recorded dependency set:

```bash
cargo install cattail --locked
```

For local development from this checkout:

```bash
make install
```

To remove a local install:

```bash
make uninstall
```

## Quick Start

```bash
cattail /var/log/syslog /var/log/auth.log
cattail -n 100 '/var/log/*.log'
cattail --since-now /tmp/a.log /tmp/b.log
cattail --prefix relative --interval-ms 100 '/var/log/*.log'
```

If you pass a glob, quote it so `cattail` performs the expansion.

## Behavior Summary

- every emitted line is prefixed with a source label
- labels default to the shortest unique basename suffix
- `--prefix relative` uses relative paths when possible
- `--prefix full` always uses full paths
- startup files print the requested backlog and then continue following
- `--since-now` skips the backlog and follows from the current end
- brand-new matching files discovered after launch start from the beginning of
  their current contents
- truncation resets the read offset to the start of the file
- delete/recreate is treated as a fresh file when the disappearance is observed

## Release Artifacts

Generated man pages and shell completions live under `packaging/`.

Regenerate them with:

```bash
make man
make completions
```

## Demo

Run the smoke demo to see backlog, live appends, truncation, delete/recreate,
and dynamic discovery:

```bash
scripts/smoke_cattail.sh
```

Set `CATTAIL_SMOKE_BURST=1` to add a short burst phase to the demo.

## Documentation

- [Operator/runtime behavior](docs/cattail.md)
- [Release checklist](docs/release.md)
- [Dependency and generated-asset provenance](docs/provenance.md)
- [Changelog](CHANGELOG.md)
- [Smoke script](scripts/smoke_cattail.sh)

## Development and support

The current development baseline is Rust 1.85 or newer. Default checks are
local Rust builds and filesystem fixtures; they do not require a service,
container, dataset, or special hardware.

This is a personal utility built around the author's own log-following workflow.
Focused correctness fixes and reproducible filesystem-behavior reports are
useful, but no support or response-time commitment is implied.

## Limitations

- polling remains part of the recovery path, so latency is bounded by
  `--interval-ms` when notify is silent or ambiguous
- exact ordering across different files is best effort
- no filtering, highlighting, panes, JSON output, or TUI
- discovery is limited to the input set provided at startup
