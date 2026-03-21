# cattail

`cattail` is a small multi-file tail utility for log-style files.

## What it does

- Expands positional arguments as file paths or glob patterns at startup
- Deduplicates the resolved file set
- Prints the last `N` lines from each file first
- Follows appended content across all files concurrently
- Prefixes each output line with a source label derived from the file name
- Handles basic truncation and recreate/reopen cases by polling the file state

## Usage

```bash
cattail ~/.local/share/orcas/logs/*.log
cattail -n 100 ~/.local/share/orcas/logs/*.log
cattail -n 0 ~/.local/share/orcas/logs/*.log
cattail /tmp/a.log /tmp/b.log
cattail 'logs/*.log'
```

Optional color flag:

```bash
cattail --color auto logs/*.log
cattail --color always logs/*.log
cattail --color never logs/*.log
```

## Current limitations

- Startup discovery only: new files that start matching an existing glob after launch are not discovered
- Follow mode uses periodic polling rather than filesystem notifications
- Output is line-oriented; partial lines are buffered until newline in live mode
- This is not a full GNU `tail -F` replacement

## Implementation note

The MVP is organized as:

- `cli.rs` for parsing
- `resolve.rs` for glob expansion and deduplication
- `tail.rs` for initial backlog extraction
- `follow.rs` for per-file polling, truncation, and reopen handling
- `output.rs` for stable label selection and serialized stdout writes

The main tradeoff is polling instead of `notify`. That keeps the code small and predictable while still handling append, truncate, disappear, and reappear cases well enough for active logs.
