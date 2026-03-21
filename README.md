# cattail

`cattail` tails multiple log files and glob matches at once.

It resolves inputs at startup, prints the last `N` lines from each resolved file, then follows appended lines live with a source prefix on every output line.
It also watches for new files that start matching a glob after launch and attaches them automatically.
Watch roots are chosen narrowly from the input shape, and repeated filesystem events are coalesced so overlapping inputs do not duplicate output.

## Quick Start

```bash
cargo run -- ~/.local/share/orcas/logs/*.log
cargo run -- -n 100 --prefix relative --interval-ms 100 logs/*.log
cargo run -- --since-now /tmp/a.log /tmp/b.log
```

## Demo Script

Run the smoke demo to see backlog, live appends, truncation, delete/recreate, and dynamic discovery behavior:

```bash
scripts/smoke_cattail.sh
```

Set `CATTAIL_SMOKE_BURST=1` to add a short burst phase to the demo.

## Docs

- [Usage and behavior](docs/cattail.md)
- [Smoke script](scripts/smoke_cattail.sh)
