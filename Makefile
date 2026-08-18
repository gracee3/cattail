RELEASE_ASSETS := tools/release-assets/Cargo.toml

.PHONY: build test fmt fmt-check clippy doc man completions smoke package install uninstall clean distclean

build:
	cargo build --locked

test:
	cargo test --locked --all-targets

fmt:
	cargo fmt

fmt-check:
	cargo fmt --check

clippy:
	cargo clippy --locked --all-targets --all-features -- -D warnings

doc:
	cargo doc --locked --no-deps

man:
	cargo run --locked --quiet --manifest-path $(RELEASE_ASSETS) -- man packaging

completions:
	cargo run --locked --quiet --manifest-path $(RELEASE_ASSETS) -- completions packaging

smoke:
	scripts/smoke_cattail.sh

package:
	cargo package --locked

install:
	cargo install --path . --locked --force

uninstall:
	cargo uninstall cattail

clean:
	cargo clean

distclean: clean
