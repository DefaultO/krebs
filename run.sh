#!/usr/bin/env bash
# For developers on macOS and Linux: rebuilds Krebs if the code changed, creates the world on first run, starts the server.
# run.bat does the same on Windows.
# Extra arguments go to the server, for example: ./run.sh --tick-rate 0
set -euo pipefail
cd "$(dirname "$0")"
ROOT="$PWD"

if ! command -v cargo >/dev/null 2>&1; then
    [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
fi
if ! command -v cargo >/dev/null 2>&1; then
    echo "Rust was not found, so Krebs cannot be built."
    echo "Get Rust from https://rustup.rs, then open a new terminal and run this again."
    [ "$(uname)" = Darwin ] && echo "On macOS it also needs the command line tools: xcode-select --install"
    exit 1
fi

# With mods here, a build lists them in both files, and neither list belongs in a commit. README.md says how to undo this.
if [ -d .git ] && [ -f mods/bare/Cargo.toml ]; then
    git -c safe.directory='*' update-index --skip-worktree crates/modlist/Cargo.toml >/dev/null 2>&1 || true
    git -c safe.directory='*' update-index --skip-worktree Cargo.lock >/dev/null 2>&1 || true
fi

# tools/v8-env.sh picks the V8 library and the features that go with it.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target/fast}"
. tools/v8-env.sh
EXE="$CARGO_TARGET_DIR/release/krebs"

# Page files are read from here on every request, so a reconnect shows an edit.
export KREBS_PAGES_DIR="$ROOT/mods/bare"
MODS=()
[ -f mods/bare/Cargo.toml ] && MODS=(--mods bare)

# The first build after the mods folder changes stops on purpose (crates/modlist/build.rs), and the second compiles them in.
build() { cargo build --release -p server $KREBS_V8_FEATURES -j 4; }
if ! build && ! build; then
    echo "Build failed, server not started."
    exit 1
fi

[ -f data/world.json ] || "$EXE" new --world 2x2 --seed 1 --data data

echo
echo "Connect the Screeps client to 127.0.0.1, port 21025. Press Ctrl+C to stop and save."
exec "$EXE" serve --data data ${MODS[@]+"${MODS[@]}"} "$@"
