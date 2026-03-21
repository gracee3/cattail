# cattail release checklist

This file is a lightweight checklist for maintainers preparing a release.

## Regenerate Release Assets

```bash
make man
make completions
```

Generated files are written under `packaging/`.

## Local Validation

```bash
make fmt
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

## Install Check

```bash
make install
cattail --help
make uninstall
```

