#!/usr/bin/env bash
# macOS and Linux side of tools/package.bat: builds the download for players and leaves it in dist/.
# A release is built from a fresh clone at its tag, so that it holds what that commit says and nothing else. So this stops
# at what a clone would not have: uncommitted changes, changes git status hides, a submodule that differs from its link,
# or a mod folder that is not in the repository. KREBS_ALLOW_DIRTY=1 lets a trial build through, which is not for publishing.
# It then builds in a copy of this folder with no world, old build or mods/platform, which is private, and the mod list in
# crates/modlist/Cargo.toml stays as it is here. Nothing else in this folder changes.
# The build goes to CARGO_TARGET_DIR if that is set, otherwise to target/package. The files that go beside the program are
# in tools/package/.
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT="$PWD"
GIT=(git -c safe.directory='*')

if ! command -v cargo >/dev/null 2>&1; then
    echo "Rust was not found. Get it from https://rustup.rs"
    exit 1
fi
for m in bare matchrun; do
    if [ ! -f "mods/$m/Cargo.toml" ]; then
        echo "mods/$m is missing, and players cannot have a match without it. Run: git submodule update --init"
        exit 1
    fi
done

# Stops unless this folder is one commit and the commits it links to, and lists what is not.
none_of() {
    if [ -n "$2" ]; then
        echo "$1, so a build here would hold what no commit says:"
        echo "$2"
        echo "A release is built from a fresh clone at its tag. For a trial build, set KREBS_ALLOW_DIRTY=1."
        exit 1
    fi
}
committed() {
    if ! command -v git >/dev/null 2>&1; then
        echo "git was not found, so nothing here can say what is in this folder. For a trial build, set KREBS_ALLOW_DIRTY=1."
        exit 1
    fi
    if ! "${GIT[@]}" rev-parse --verify HEAD >/dev/null 2>&1; then
        echo "This folder is not a git clone, so nothing can say what is in it. A release is built from a fresh clone at its tag."
        exit 1
    fi
    none_of "Uncommitted changes" "$("${GIT[@]}" status --porcelain --ignore-submodules=none)"
    # A submodule status starts with a space when it matches its link.
    none_of "Submodules that differ from their links" "$("${GIT[@]}" submodule status --recursive | grep -v '^ ' || true)"
    # git status cannot see into these: run.sh marks Cargo.lock and the mod list this way.
    none_of "Files git status has been told to skip" "$("${GIT[@]}" ls-files -v | grep -E '^(S|h) ' || true)"
    # crates/modlist/build.rs compiles in every folder here. mods/platform stays out of the copy, so it cannot get in.
    local stray=""
    for d in mods/*/; do
        d="${d%/}"
        [ "$(basename "$d")" = platform ] && continue
        [ -n "$("${GIT[@]}" ls-tree --name-only HEAD "$d")" ] || stray="$stray$d"$'\n'
    done
    none_of "Mod folders that are not in the repository" "${stray%$'\n'}"
}
if [ "${KREBS_ALLOW_DIRTY:-}" = 1 ]; then
    echo "KREBS_ALLOW_DIRTY is set, so this build may hold what no commit has. Do not publish it."
else
    committed
fi

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
if [ -z "$VERSION" ]; then
    echo "No version found in Cargo.toml."
    exit 1
fi
case "$(uname -s)" in
    Darwin) OS=macos ;;
    Linux) OS=linux ;;
    *) echo "tools/package.sh builds for macOS and Linux. Use tools\\package.bat on Windows."; exit 1 ;;
esac
case "$(uname -m)" in
    arm64 | aarch64) ARCH=arm64 ;;
    x86_64 | amd64) ARCH=x64 ;;
    *) ARCH="$(uname -m)" ;;
esac
NAME="Krebs-v$VERSION-$OS-$ARCH"

export CARGO_TARGET_DIR="$(mkdir -p "${CARGO_TARGET_DIR:-$ROOT/target/package}" && cd "${CARGO_TARGET_DIR:-$ROOT/target/package}" && pwd)"
SOURCE="$CARGO_TARGET_DIR/source"
STAGE_ROOT="$CARGO_TARGET_DIR/stage"
# Everything in the archive sits in one folder, so that unpacking it in Downloads does not leave krebs among the downloads.
STAGE="$STAGE_ROOT/Krebs"

echo "Cleaning."
rm -rf "$SOURCE" "$STAGE_ROOT" "$CARGO_TARGET_DIR/dist/krebs"
mkdir -p "$STAGE" "$SOURCE"

echo "Copying the source."
rsync -a --exclude=.git \
    --exclude=/.claude --exclude=/target --exclude=/data --exclude=/reference --exclude=/dist --exclude=/mods/platform \
    --exclude=/vendor/v8/build --exclude=/vendor/v8/buildtools --exclude=/vendor/v8/third_party --exclude=/vendor/v8/tools \
    --exclude=/vendor/v8/v8 \
    "$ROOT/" "$SOURCE/"
# run.sh leaves the mods of this machine listed here, and a listed mod that was not copied would stop Cargo before build.rs runs.
LIST="$SOURCE/crates/modlist/Cargo.toml"
grep -v '\.\./\.\./mods/' "$LIST" > "$LIST.new" || true
mv "$LIST.new" "$LIST"

echo "Building. A first build takes several minutes."
bash "$SOURCE/tools/dist.sh"

cp "$CARGO_TARGET_DIR/dist/krebs" "$STAGE/krebs"

# The notices name the crates in Rust's standard library by pointing at this file.
if ! cp "$(rustc --print sysroot)/share/doc/rust/COPYRIGHT-library.html" "$STAGE/RUST_NOTICES.html"; then
    echo "The Rust toolchain has no COPYRIGHT-library.html, so it looks damaged. Reinstall it with rustup."
    exit 1
fi

cp "$ROOT/tools/package/krebs.json" "$STAGE/"
# .command opens in Terminal on a double-click in Finder. Linux file managers run a .sh.
if [ "$OS" = macos ]; then
    cp "$ROOT/tools/package/Let friends join.command" "$STAGE/"
else
    cp "$ROOT/tools/package/Let friends join.command" "$STAGE/Let friends join.sh"
fi
chmod +x "$STAGE/krebs" "$STAGE"/Let\ friends\ join.*
mkdir "$STAGE/starter-bot"
cp "$ROOT/tools/package/starter-bot/main.js" "$STAGE/starter-bot/"
cp "$ROOT/tools/package/README.txt" "$STAGE/README.txt"
# README.txt is the Windows one. This page says what is different here, and that this build is not a tested release.
cp "$ROOT/tools/package/README-macos-linux.txt" "$STAGE/README-macos-linux.txt"
cp "$ROOT/LICENSE" "$STAGE/LICENSE.txt"
cp "$ROOT/THIRD_PARTY_NOTICES.md" "$STAGE/THIRD_PARTY_NOTICES.txt"

# The program has to start, has to need nothing a player may not have, and has to hold what a match needs.
if ! "$STAGE/krebs" --help >/dev/null; then
    echo "The new krebs does not start."
    exit 1
fi
if [ "$OS" = macos ]; then
    extra="$(otool -L "$STAGE/krebs" | tail -n +2 | awk '{print $1}' | grep -vE '^(/usr/lib/|/System/Library/)' || true)"
    none_of "The new krebs links libraries outside macOS itself" "$extra"
fi
for s in /api/lobbies /api/matchrun/view; do
    if ! grep -qa "$s" "$STAGE/krebs"; then
        echo "The new krebs does not have $s, so a mod is not in it."
        exit 1
    fi
done

# Only now: a build that stops on the way leaves the last archive where it was.
rm -rf "$ROOT/dist"
mkdir "$ROOT/dist"
if [ "$OS" = macos ]; then
    ARCHIVE="$ROOT/dist/$NAME.zip"
    ditto -c -k --keepParent "$STAGE" "$ARCHIVE"
    unzip -l "$ARCHIVE"
else
    ARCHIVE="$ROOT/dist/$NAME.tar.gz"
    tar -C "$STAGE_ROOT" -czf "$ARCHIVE" Krebs
    tar -tvzf "$ARCHIVE"
fi
echo "Built $ARCHIVE, $(wc -c < "$ARCHIVE" | tr -d ' ') bytes"
