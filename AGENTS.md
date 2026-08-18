# Contributor and agent guidance

`cattail` is a small Rust command-line utility for tailing multiple files and
glob patterns with labeled output, dynamic discovery, and recovery from
truncation or replacement. Keep the scope on reliable filesystem-follow
behavior; do not turn it into a log service, parser framework, or TUI.

Before changing code, read `README.md`, `docs/cattail.md`, `docs/release.md`, and
`docs/provenance.md`. Read `CHANGELOG.md` for published and unreleased behavior.

## Validation

The ordinary reviewed checks are:

```bash
make test
git diff --check
```

For Rust, CI, packaging, generated assets, or release-facing changes, also run:

```bash
make fmt-check
make clippy
make doc
make man completions
git diff --exit-code -- packaging
make package
```

Use locked dependencies. Keep the declared minimum Rust version and CI
toolchains deliberate. Do not publish, tag, or alter released artifacts as part
of ordinary validation.

## Safety, provenance, and delivery

- Tests use temporary fictional filesystem fixtures. Never commit real logs,
  host paths, credentials, process captures, or identifying machine data.
- Preserve bounded memory, line framing, prefix uniqueness, truncation,
  delete/recreate, dynamic discovery, and graceful shutdown behavior. Add
  failure-oriented tests when those contracts change.
- Keep license, dependency provenance, generated man pages/completions,
  changelog, crate metadata, and public release claims aligned.
- Use a focused feature branch. Commit and push the validated change and open a
  pull request; incomplete or higher-risk work stays draft. Do not treat local
  files as delivered work.
- After publication, send the exact commit, PR, validation, outcome, risks, and
  next action to the repository's external coordination record. Do not claim
  completion until that remote handoff is verified.
