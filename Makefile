.PHONY: sweep sweep-dry-run

# Keep recently used Cargo artifacts, including the binaries and dependencies used by daily builds.
# cargo-sweep only visits this checkout's target directory; it does not clear ~/.cargo downloads.
# Run between builds and tests so artifacts are not removed while Cargo is using them.
SWEEP_DAYS := 3
REPO_ROOT := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))

sweep:
	@command -v cargo-sweep >/dev/null || { echo 'cargo-sweep is required (install with: cargo install cargo-sweep --locked)' >&2; exit 1; }
	@cargo sweep --time $(SWEEP_DAYS) "$(REPO_ROOT)"

sweep-dry-run:
	@command -v cargo-sweep >/dev/null || { echo 'cargo-sweep is required (install with: cargo install cargo-sweep --locked)' >&2; exit 1; }
	@cargo sweep --time $(SWEEP_DAYS) --dry-run "$(REPO_ROOT)"
