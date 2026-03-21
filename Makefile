RELEASE_ASSETS := tools/release-assets/Cargo.toml

.PHONY: build test fmt clippy doc man completions smoke package install uninstall clean distclean

build:
	cargo build

test:
	cargo test

fmt:
	cargo fmt

clippy:
	cargo clippy --all-targets --all-features -- -D warnings

doc:
	cargo doc --no-deps

man:
	cargo run --quiet --manifest-path $(RELEASE_ASSETS) -- man packaging

completions:
	cargo run --quiet --manifest-path $(RELEASE_ASSETS) -- completions packaging

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
