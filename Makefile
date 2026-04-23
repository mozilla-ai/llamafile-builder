# llamafile-builder build system
#
# Prerequisites (macOS):
#   brew install zig
#   cargo install cargo-zigbuild
#   rustup target add x86_64-apple-darwin x86_64-unknown-linux-gnu \
#     aarch64-unknown-linux-gnu x86_64-pc-windows-gnu
#
# Usage:
#   make              # build for current platform (debug)
#   make release      # build for current platform (release)
#   make all          # cross-compile all 5 targets (release)
#   make test         # run all tests
#   make clean        # remove build artifacts
#   make dist         # build all targets and copy to dist/

TARGETS := \
	aarch64-apple-darwin \
	x86_64-apple-darwin \
	x86_64-unknown-linux-gnu \
	aarch64-unknown-linux-gnu \
	x86_64-pc-windows-gnu

CRATES := -p llamafile-builder -p llamafile-builder-tui

# Default: build for current platform in debug mode
.PHONY: build
build:
	cargo build $(CRATES)

# Release build for current platform
.PHONY: release
release:
	cargo build --release $(CRATES)

# Run all tests
.PHONY: test
test:
	cargo test --workspace

# Cross-compile all targets (release)
.PHONY: all
all: check-deps
	@for target in $(TARGETS); do \
		echo "=== Building $$target ==="; \
		cargo zigbuild --release --target $$target $(CRATES) || exit 1; \
	done
	@echo ""
	@echo "All targets built successfully."
	@$(MAKE) --no-print-directory sizes

# Build a single target: make target T=x86_64-unknown-linux-gnu
.PHONY: target
target: check-deps
	cargo zigbuild --release --target $(T) $(CRATES)

# Copy all release binaries to dist/
.PHONY: dist
dist: all
	@mkdir -p dist
	@for target in $(TARGETS); do \
		mkdir -p dist/$$target; \
		if echo $$target | grep -q windows; then \
			cp target/$$target/release/llamafile-builder.exe dist/$$target/ 2>/dev/null || true; \
			cp target/$$target/release/llamafile-builder-tui.exe dist/$$target/ 2>/dev/null || true; \
		else \
			cp target/$$target/release/llamafile-builder dist/$$target/ 2>/dev/null || true; \
			cp target/$$target/release/llamafile-builder-tui dist/$$target/ 2>/dev/null || true; \
		fi; \
	done
	@echo ""
	@echo "Binaries copied to dist/"
	@find dist -type f -name 'llamafile-builder*' | sort | while read f; do \
		printf "  %-60s %s\n" "$$f" "$$(du -h "$$f" | cut -f1)"; \
	done

# Show binary sizes for all targets
.PHONY: sizes
sizes:
	@for target in $(TARGETS); do \
		if echo $$target | grep -q windows; then \
			cli=$$(du -h target/$$target/release/llamafile-builder.exe 2>/dev/null | cut -f1); \
			tui=$$(du -h target/$$target/release/llamafile-builder-tui.exe 2>/dev/null | cut -f1); \
		else \
			cli=$$(du -h target/$$target/release/llamafile-builder 2>/dev/null | cut -f1); \
			tui=$$(du -h target/$$target/release/llamafile-builder-tui 2>/dev/null | cut -f1); \
		fi; \
		printf "  %-35s CLI: %s  TUI: %s\n" "$$target" "$$cli" "$$tui"; \
	done

# Verify all build dependencies are installed
.PHONY: check-deps
check-deps:
	@command -v cargo >/dev/null 2>&1 || { echo "Error: cargo not found. Install Rust: https://rustup.rs"; exit 1; }
	@command -v zig >/dev/null 2>&1 || { echo "Error: zig not found. Install: brew install zig"; exit 1; }
	@command -v cargo-zigbuild >/dev/null 2>&1 || { echo "Error: cargo-zigbuild not found. Install: cargo install cargo-zigbuild"; exit 1; }
	@for target in $(TARGETS); do \
		rustup target list --installed 2>/dev/null | grep -q "$$target" || { \
			echo "Error: Rust target $$target not installed. Run: rustup target add $$target"; exit 1; \
		}; \
	done

# Install all build dependencies (macOS)
.PHONY: setup
setup:
	@echo "Installing build dependencies..."
	@command -v rustup >/dev/null 2>&1 || { echo "Install Rust first: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"; exit 1; }
	@command -v brew >/dev/null 2>&1 && brew install zig || echo "Warning: brew not found, install zig manually"
	cargo install cargo-zigbuild
	rustup target add $(TARGETS)
	@echo "Done. Run 'make all' to build."

.PHONY: clean
clean:
	cargo clean
	rm -rf dist
