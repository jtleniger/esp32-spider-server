#!/usr/bin/env bash
#
# Build a static, glibc-independent release binary.
#
# The default `cargo build --release` links the build host's glibc dynamically:
# the resulting binary records that glibc's symbol versions and refuses to start
# on any host with an older libc, e.g.
#
#   version `GLIBC_2.34' not found (required by .../esp32-spider-server)
#
# Building for x86_64-unknown-linux-musl links musl statically, so the binary runs
# on any Linux kernel regardless of the host's glibc.
set -euo pipefail

TARGET=x86_64-unknown-linux-musl
cd "$(dirname "$0")/.."

if ! command -v musl-gcc >/dev/null 2>&1; then
    echo "error: musl-gcc not found. Install the musl C toolchain, e.g.:" >&2
    echo "  sudo apt-get install musl-tools" >&2
    exit 1
fi

rustup target add "$TARGET"
cargo build --release --target "$TARGET"

BIN="target/$TARGET/release/esp32-spider-server"
echo
file "$BIN"
echo "binary: $BIN"
