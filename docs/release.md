# cattail release checklist

This file is a lightweight checklist for maintainers preparing a release.

Version 0.1.0 is already published on crates.io and GitHub. This checklist
applies to the next release; do not reuse the v0.1.0 tag.

## Regenerate Release Assets

```bash
make man
make completions
git diff --exit-code -- packaging
```

Generated files are written under `packaging/`.

## Local Validation

```bash
make fmt-check
make clippy
make test
make doc
make smoke
```

## Packaging

```bash
make package
```

If packaging succeeds, review the crate contents before publishing.

CI also checks the declared Rust 1.85 minimum separately from the pinned current
toolchain. If either toolchain fails, change the implementation or update the
declared support boundary deliberately; do not silently float the CI toolchain.

## Install Check

```bash
make install
cattail --help
make uninstall
```

Before tagging, update `CHANGELOG.md`, verify Cargo/README/release versions agree,
and confirm the candidate commit is green. The GitHub release and crates.io
package must refer to the same source boundary even though their artifacts are
published separately.
