CARGO ?= cargo
VERSION ?= 0.1.0
TARGET ?= $(shell rustc -vV | sed -n 's/^host: //p')
BINARY ?= target/release/shimmer

.PHONY: build test terminal-test install package

build:
	SHIMMER_VERSION="$(VERSION)" $(CARGO) build --release --locked

test:
	$(CARGO) fmt --all -- --check
	$(CARGO) clippy --locked --all-targets -- -D warnings
	$(CARGO) test --locked

terminal-test: build
	python3 tests/terminal.py "$(BINARY)"

install: build
	SHIMMER_BIN="$(CURDIR)/$(BINARY)" sh scripts/install.sh

package: build
	sh scripts/package.sh "$(VERSION)" "$(TARGET)" "$(BINARY)"
