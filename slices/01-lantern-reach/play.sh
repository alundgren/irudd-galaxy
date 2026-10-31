#!/usr/bin/env bash
# The Lantern Reach: build in release (thin LTO) and play.
# Arguments go to `galaxy play`, e.g. ./play.sh --new
set -euo pipefail
cd "$(dirname "$0")"
# rustup installs into ~/.cargo/bin; a fresh shell may not have it on PATH yet.
if ! command -v cargo >/dev/null 2>&1 && [ -x "$HOME/.cargo/bin/cargo" ]; then
    export PATH="$HOME/.cargo/bin:$PATH"
fi
if ! command -v cargo >/dev/null 2>&1; then
    echo "play.sh: cargo not found. Install Rust with rustup: https://rustup.rs" >&2
    exit 1
fi
exec cargo xtask run --release -- play "$@"
