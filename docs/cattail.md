# cattail

`cattail` is a small multi-file tail utility for log-style files.

It resolves file paths and glob patterns at startup, prints a backlog window from each file, then follows appended data live with one prefixed line per emitted record.
It also watches for newly created files that match an existing glob after launch and attaches them automatically.
The watcher layer chooses narrow roots from the input shape so `logs/*.log` watches `logs/`, while patterns with nested wildcards expand to recursive watching only when needed.

## Usage

```bash
cattail ~/.local/share/orcas/logs/*.log
cattail -n 100 ~/.local/share/orcas/logs/*.log
cattail -n 0 ~/.local/share/orcas/logs/*.log
cattail --since-now ~/.local/share/orcas/logs/*.log
cattail --prefix relative --interval-ms 100 /tmp/a.log /tmp/b.log
cattail 'logs/*.log'
```

## Flags

- `-n, --lines <N>`: backlog line count, default `50`
- `--since-now`: skip the backlog entirely and only emit new lines after startup
- `--interval-ms <N>`: polling interval in milliseconds, default `200`
- `--prefix basename|relative|full`: label format for each line
- `--color auto|always|never`: optional ANSI color for prefixes

`--since-now` wins over `--lines` when both are supplied.

## Follow Model

`cattail` uses a hybrid model:

- `notify` wakes workers promptly when the filesystem reports a change
- a lightweight polling scan remains in place as a recovery path so discovery still works if a backend misses an event
- each file has its own worker, and a single printer serializes all stdout writes

That means:

- new appended lines appear promptly after a notify event, or on the next recovery scan/tick
- partial lines stay buffered until a newline arrives
- output is serialized through a single printer so lines do not interleave
- repeated notify bursts are coalesced by path before attachment or wakeup, so overlapping globs do not attach the same file twice
- a temporarily missing or unreadable file emits one concise stderr notice and is retried on later ticks

## Prefix Modes

- `basename`: default; uses the shortest unique suffix ending in the file name
- `relative`: uses a path relative to the current working directory when possible
- `full`: uses an absolute path

If two files would produce the same label in `relative` mode, `cattail` widens the colliding labels to a full path form for those entries.

## Truncation and Recreate Policy

The follow loop keeps a byte offset for each file.

- If a file shrinks in place, `cattail` treats that as truncation, resets the offset to `0`, and continues from the new beginning of the file.
- If a watched file disappears and later reappears at the same path, `cattail` treats the reappearance as a fresh file and starts reading it from the beginning on the first successful poll.
- If a brand-new file appears after launch and matches an existing glob, `cattail` attaches it and starts reading from the beginning of its current contents.

That behavior is deliberate and tested.

## Current Limitations

- Dynamic discovery is limited to the input set provided at startup; new glob patterns are not added after launch
- Polling remains part of the recovery path, so latency is bounded by `--interval-ms` when notify is silent or ambiguous
- This is not a full GNU `tail -F` clone
- No filtering, JSON output, panes, or TUI
- Exact ordering between different files is best effort only; per-file ordering is preserved, but inter-file ordering depends on event timing and scheduling

## Known Edge Conditions

- Some filesystem backends coalesce or drop very noisy bursts; `cattail` treats notify as a wakeup path and uses polling as recovery, but extremely transient changes can still be observed on the next scan tick rather than instantly
- Newly discovered files that already contain history are intentionally read from the beginning of their current contents, so the first attach may replay pre-existing lines once
- Delete/recreate is intentionally treated as a fresh file once the disappearance is observed; if a backend misses the missing window entirely, recovery still converges on the current file contents
- Very broad globs are still narrowed by the watcher planner, but the tool is not attempting full filesystem indexing or perfect tail parity

## Watch Roots

`cattail` plans watcher roots from the original inputs instead of watching a broad top-level directory.

- literals watch their parent directory
- simple globs such as `logs/*.log` watch the anchor directory non-recursively
- globs with wildcards in directory segments or `**` use recursive watching from the closest stable anchor
- overlapping roots are merged so a recursive root can cover narrower descendants

This keeps notify traffic narrower on larger trees while preserving discovery of new matching files.

## Smoke Demo

The smoke script exercises the product end to end:

```bash
scripts/smoke_cattail.sh
```

It creates temporary log files, seeds backlog lines, launches `cattail`, appends new lines, truncates one file, deletes/recreates another, and creates a brand-new matching file after launch so you can observe the current lifecycle behavior in one run.

Set `CATTAIL_SMOKE_BURST=1` to add a short burst phase that rapidly appends several extra lines to one file.

## Implementation Note

Module layout:

- `cli.rs`: CLI parsing and config
- `resolve.rs`: glob expansion and deduplication
- `tail.rs`: last-N backlog extraction
- `follow.rs`: per-file polling, truncation, and reopen handling
- `watch.rs`: notify-backed coordination, dynamic discovery, and worker lifecycle
- `output.rs`: prefix selection and serialized stdout writing

The main tradeoff is keeping a small polling fallback alongside `notify`. That keeps the code compact and predictable while still being reliable enough for active log use in this v2 slice.
