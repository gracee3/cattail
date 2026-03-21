# cattail operator guide

`cattail` is a small multi-file tail utility for log-style files.

It resolves file paths and glob patterns at startup, prints a backlog window
from each resolved file, then follows appended data live with one prefixed line
per completed record.

It also watches for newly created files that match an existing startup glob and
attaches them automatically.

## Core Model

- literal paths are resolved at startup and followed directly
- glob patterns are expanded at startup and deduped against each other
- each resolved file gets its own follow worker
- all completed records are serialized through a single stdout printer

## Follow Semantics

`cattail` uses a hybrid follow model:

- `notify` wakes workers promptly when the filesystem reports a change
- a lightweight polling scan remains in place as a recovery path so discovery
  still works if a backend misses an event
- repeated notify bursts are coalesced by path before attachment or wakeup
- a temporarily missing or unreadable file emits one concise stderr notice and
  is retried on later ticks

This keeps the runtime responsive while still converging on the current file
state when event delivery is noisy or incomplete.

## Backlog and `--since-now`

At startup, `cattail` resolves the input set and then:

- prints the last `N` lines from each startup-resolved file when `-n, --lines`
  is used
- skips the backlog entirely when `--since-now` is present

For startup-resolved files, `--since-now` is the explicit form of "start
following from the current end". Using `-n 0` reaches the same runtime state,
but `--since-now` makes the intent clearer.

## Prefix Modes

- `basename`: default; uses the shortest unique suffix ending in the file name
- `relative`: uses a path relative to the current working directory when
  possible
- `full`: uses an absolute path

If two files would produce the same label in `relative` mode, `cattail` widens
the colliding labels to a full path form for those entries.

## Truncation and Recreate Policy

The follow loop keeps a byte offset for each file.

- if a file shrinks in place, `cattail` treats that as truncation, resets the
  offset to `0`, and continues from the new beginning of the file
- if a watched file disappears and later reappears at the same path, `cattail`
  treats the reappearance as a fresh file and starts reading it from the
  beginning on the first successful poll
- if a brand-new file appears after launch and matches an existing glob,
  `cattail` attaches it and starts reading from the beginning of its current
  contents

That behavior is deliberate and tested.

## Watch-Root Planning

`cattail` plans watcher roots from the original inputs instead of watching a
broad top-level directory.

- literals watch their parent directory
- simple globs such as `logs/*.log` watch the anchor directory non-recursively
- globs with wildcards in directory segments or `**` use recursive watching
  from the closest stable anchor
- overlapping roots are merged so a recursive root can cover narrower
  descendants

This keeps notify traffic narrower on larger trees while preserving discovery of
new matching files.

## Dynamic Discovery

Dynamic discovery is intentionally limited to the startup input set.

- new matching files are attached automatically if they appear later
- already-attached paths are never attached twice
- overlapping glob patterns are deduped before follow workers are spawned
- newly discovered files begin reading from the beginning of their current
  contents

## Current Limitations

- discovery is limited to the input set provided at startup; new glob patterns
  are not added after launch
- polling remains part of the recovery path, so latency is bounded by
  `--interval-ms` when notify is silent or ambiguous
- exact ordering between different files is best effort only; per-file ordering
  is preserved, but inter-file ordering depends on event timing and scheduling
- `cattail` is not a full GNU `tail -F` clone
- no filtering, JSON output, panes, or TUI

## Known Edge Conditions

- some filesystem backends coalesce or drop very noisy bursts; `cattail`
  treats notify as a wakeup path and uses polling as recovery, but extremely
  transient changes can still be observed on the next scan tick rather than
  instantly
- newly discovered files that already contain history are intentionally read
  from the beginning of their current contents, so the first attach may replay
  pre-existing lines once
- delete/recreate is intentionally treated as a fresh file once the
  disappearance is observed; if a backend misses the missing window entirely,
  recovery still converges on the current file contents
- very broad globs are still narrowed by the watcher planner, but the tool is
  not attempting full filesystem indexing or perfect tail parity

## Smoke Demo

The smoke script exercises the product end to end:

```bash
scripts/smoke_cattail.sh
```

It creates temporary log files, seeds backlog lines, launches `cattail`,
appends new lines, truncates one file, deletes/recreates another, and creates a
brand-new matching file after launch so you can observe the current lifecycle
behavior in one run.

Set `CATTAIL_SMOKE_BURST=1` to add a short burst phase that rapidly appends
several extra lines to one file.

## Troubleshooting

- quote glob patterns when invoking `cattail` from a shell so the program owns
  expansion
- if output seems delayed, reduce `--interval-ms`
- if you expect new matching files to appear, confirm they are covered by the
  startup glob set
- if a file is deleted and recreated very quickly, recovery depends on whether
  the backend exposes the missing window before the next poll tick

## Implementation Note

Module layout:

- `cli.rs`: CLI parsing and config
- `resolve.rs`: glob expansion and deduplication
- `tail.rs`: last-N backlog extraction
- `follow.rs`: per-file polling, truncation, and reopen handling
- `watch.rs`: notify-backed coordination, dynamic discovery, and worker
  lifecycle
- `output.rs`: prefix selection and serialized stdout writing

The main tradeoff is keeping a small polling fallback alongside `notify`. That
keeps the code compact and predictable while still being reliable enough for
active log use in this release slice.
