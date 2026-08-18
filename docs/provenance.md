# Dependency and generated-asset provenance

`cattail` is original project code released under the repository's MIT license.
Its direct Rust dependencies are obtained from crates.io and retain their own
licenses:

- `anyhow` — error context;
- `clap` — command-line parsing;
- `glob` — startup and discovery pattern matching;
- `notify` — platform filesystem notifications;
- `tokio` — async file following, timers, signals, and worker coordination;
- `tempfile` — test-only filesystem fixtures.

`Cargo.lock` records the exact dependency graph used by CI and `cargo install
--locked`. Review upstream license metadata whenever that graph changes; the
repository's MIT license does not relicense dependencies.

Files beneath `packaging/man/` and `packaging/completions/` are generated from
the `cattail` CLI definition by the source-controlled
`tools/release-assets` helper. CI regenerates them and rejects drift. They do not
contain copied shell-completion or manual-page sources from another project.
