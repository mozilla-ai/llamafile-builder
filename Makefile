# llamafile-builder build system
#
# Prerequisites:
#   Rust toolchain (https://rustup.rs)
#
# Run `make setup` once to install zig, cargo-zigbuild, and the cross-compile
# Rust targets. Works on macOS (via Homebrew) and Linux (downloads zig from
# ziglang.org into $$HOME/.local/bin — make sure that is on your PATH).
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

# Pinned zig version installed by `make setup` on Linux. Bump when
# cargo-zigbuild requires a newer toolchain.
ZIG_VERSION := 0.16.0

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

# Cross-compile all targets (release).
#
# On macOS, *-apple-darwin targets are built with the native Apple toolchain
# (`cargo build`) — zig's `ar` wrapper has shown spurious archive-creation
# failures with `ring` on some macOS versions, and Apple's clang+ld handle
# both arm64 and x86_64 darwin natively. zigbuild is still used for the
# linux-gnu and windows-gnu targets, where it is the whole reason this works.
#
# The `zigbuild-ar` shell snippet (below) resolves a working AR for the
# zigbuild branch and exports AR_<target>: on macOS 26 (Tahoe), zig 0.16.0's
# bundled llvm-ar (LLVM 21) fails to create archives at all (`ring` dies with
# `ar: error: unable to open … No such file or directory`). Homebrew's
# standalone llvm-ar handles the same inputs fine. On other macOS versions
# the override is harmless. On Linux, zig ar works, so it's a no-op.
ZIGBUILD_AR_SETUP := \
	if [ "$$(uname -s)" = "Darwin" ]; then \
		prefix=$$(brew --prefix llvm 2>/dev/null || true); \
		if [ -z "$$prefix" ] || [ ! -x "$$prefix/bin/llvm-ar" ]; then \
			echo "Error: Homebrew llvm-ar not found. Install with: brew install llvm" >&2; \
			echo "(Workaround for zig 0.16.0 bundled llvm-ar bug on macOS 26.)" >&2; \
			exit 1; \
		fi; \
		ar_var=$$(echo "AR_$$ar_target" | tr '-' '_'); \
		export $$ar_var="$$prefix/bin/llvm-ar"; \
	fi

.PHONY: all
all: check-deps
	@for target in $(TARGETS); do \
		echo "=== Building $$target ==="; \
		if [ "$$(uname -s)" = "Darwin" ] && echo "$$target" | grep -q apple-darwin; then \
			cargo build --release --target $$target $(CRATES) || exit 1; \
		else \
			ar_target=$$target; $(ZIGBUILD_AR_SETUP) && \
			cargo zigbuild --release --target $$target $(CRATES) || exit 1; \
		fi; \
	done
	@echo ""
	@echo "All targets built successfully."
	@$(MAKE) --no-print-directory sizes

# Build a single target: make target T=x86_64-unknown-linux-gnu
.PHONY: target
target: check-deps
	@if [ "$$(uname -s)" = "Darwin" ] && echo "$(T)" | grep -q apple-darwin; then \
		cargo build --release --target $(T) $(CRATES); \
	else \
		ar_target=$(T); $(ZIGBUILD_AR_SETUP) && \
		cargo zigbuild --release --target $(T) $(CRATES); \
	fi

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

# Install all build dependencies (macOS via Homebrew, Linux via direct download).
.PHONY: setup
setup: setup-zig
	@command -v rustup >/dev/null 2>&1 || { echo "Install Rust first: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"; exit 1; }
	@command -v cargo-zigbuild >/dev/null 2>&1 || cargo install cargo-zigbuild
	rustup target add $(TARGETS)
	@echo "Done. Run 'make all' to build."

# Install zig pinned to $(ZIG_VERSION). macOS uses Homebrew; Linux downloads
# the official tarball from ziglang.org into $$HOME/.local/bin. If a different
# zig is already on PATH, prompts the user (default: keep current); in
# non-interactive environments (CI) it installs the pinned version automatically.
.PHONY: setup-zig
setup-zig:
	@installed=$$(command -v zig >/dev/null 2>&1 && zig version || true); \
	if [ "$$installed" = "$(ZIG_VERSION)" ]; then \
		echo "zig $(ZIG_VERSION) already installed"; \
		exit 0; \
	fi; \
	if [ -n "$$installed" ]; then \
		echo "zig $$installed is on PATH but the pinned version is $(ZIG_VERSION)."; \
		if [ -t 0 ]; then \
			printf "Install pinned zig $(ZIG_VERSION) [y/N]? "; \
			read ans; \
		else \
			ans=y; \
		fi; \
		case "$$ans" in \
			[yY]*) ;; \
			*) echo "Keeping zig $$installed (cross-compile builds will use this version)."; exit 0 ;; \
		esac; \
	fi; \
	uname_s=$$(uname -s); \
	if [ "$$uname_s" = "Darwin" ]; then \
		command -v brew >/dev/null 2>&1 || { echo "Install Homebrew first: https://brew.sh"; exit 1; }; \
		brew install zig; \
	elif [ "$$uname_s" = "Linux" ]; then \
		arch=$$(uname -m); \
		case "$$arch" in \
			x86_64) zig_arch=x86_64 ;; \
			aarch64|arm64) zig_arch=aarch64 ;; \
			*) echo "Unsupported Linux arch: $$arch"; exit 1 ;; \
		esac; \
		dir="zig-$$zig_arch-linux-$(ZIG_VERSION)"; \
		url="https://ziglang.org/download/$(ZIG_VERSION)/$$dir.tar.xz"; \
		echo "Downloading $$url"; \
		mkdir -p $$HOME/.local/share $$HOME/.local/bin; \
		tmp=$$(mktemp -d); \
		curl -fsSL "$$url" -o "$$tmp/zig.tar.xz"; \
		tar -xf "$$tmp/zig.tar.xz" -C "$$tmp"; \
		rm -rf "$$HOME/.local/share/zig-$(ZIG_VERSION)"; \
		mv "$$tmp/$$dir" "$$HOME/.local/share/zig-$(ZIG_VERSION)"; \
		ln -sf "$$HOME/.local/share/zig-$(ZIG_VERSION)/zig" "$$HOME/.local/bin/zig"; \
		rm -rf "$$tmp"; \
		echo "Installed zig to $$HOME/.local/bin/zig"; \
		case ":$$PATH:" in *":$$HOME/.local/bin:"*) ;; *) echo "WARNING: $$HOME/.local/bin is not on PATH — add it before running 'make all'." ;; esac; \
	else \
		echo "Unsupported OS: $$uname_s — install zig manually."; exit 1; \
	fi

.PHONY: clean
clean:
	cargo clean
	rm -rf dist
