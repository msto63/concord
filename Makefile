# Concord — developer tasks.
.PHONY: test smokes build clippy version

# Full suite: cargo unit/integration tests + all shell smoke tests.
test:
	bash tests/all.sh --with-cargo

# Just the shell smoke tests (auto-discovered).
smokes:
	bash tests/all.sh

build:
	cargo build --workspace --all-targets

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

version:
	bash scripts/check-version.sh
