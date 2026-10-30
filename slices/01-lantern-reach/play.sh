#!/bin/sh
# The Lantern Reach: build in release (thin LTO) and play.
# Arguments go to `galaxy play`, e.g. ./play.sh --new
set -e
cd "$(dirname "$0")"
cargo build --release -p galaxy-app
exec ./target/release/galaxy play "$@"
