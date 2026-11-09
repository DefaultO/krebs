#!/usr/bin/env bash
# macOS and Linux side of tools/dist.bat: builds the krebs that goes to players, with every mod that is checked out in mods/.
# It ends up in dist/ under CARGO_TARGET_DIR, or in target/dist-exe/dist/ if that is not set. tools/package.sh calls this and
# makes the archive around it.
# No path of this machine may be in it. The dist profile drops the debug information, and the remaps below cover what is
# left: crates from the registry name their own folder in panic messages.
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT="$PWD"

if [ -f mods/platform/Cargo.toml ]; then
    echo "mods/platform is private, and every mod found here gets built in. Use tools/package.sh, which leaves it out."
    exit 1
fi
if ! command -v cargo >/dev/null 2>&1; then
    echo "Rust was not found. Get it from https://rustup.rs"
    exit 1
fi

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target/dist-exe}"
. tools/v8-env.sh

CARGO_FOLDER="${CARGO_HOME:-$HOME/.cargo}"
case "$CARGO_FOLDER$CARGO_TARGET_DIR$ROOT" in
    *"'"*)
        echo "Krebs cannot be built in a folder with an apostrophe in its name. Rename the folder or move Krebs, then run this again."
        exit 1
        ;;
esac
REMAPS="build.rustflags=['--remap-path-prefix=$CARGO_FOLDER=cargo','--remap-path-prefix=$CARGO_TARGET_DIR=target','--remap-path-prefix=$ROOT=krebs']"
# Either of these would win over the remaps.
unset RUSTFLAGS CARGO_ENCODED_RUSTFLAGS

# The first build after the mods folder changes stops on purpose (crates/modlist/build.rs), and the second compiles them in.
build() { cargo build --config "$REMAPS" --profile dist -p server $KREBS_V8_FEATURES -j 4; }
if ! build && ! build; then
    echo "Build failed."
    exit 1
fi
echo "Built $CARGO_TARGET_DIR/dist/krebs"
