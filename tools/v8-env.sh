# Sourced by run.sh and tools/dist.sh on macOS and Linux: picks the V8 library and sets KREBS_V8_FEATURES to match.
# tools/v8-download.bat is the Windows side.
#
# runtime/ptrcomp needs a V8 with pointer compression and a 4 GB cage per isolate (docs/ARCHITECTURE.md 2.9). The fast
# library tools/v8-download.bat fetches is one, but it is built for Windows only. The ptrcomp prebuilts on
# denoland/rusty_v8 share one 4 GB cage among all isolates of the process, and a server with many bots runs out of it
# ("Fatal JavaScript out of memory"), so they must not be used. Elsewhere the order is:
#   1. A library built on this machine with pointer compression and a cage per isolate, at
#      target/ptrcomp/release/gn_out, the place tools/v8-ptrcomp.ps1 uses: built with runtime/ptrcomp.
#   2. RUSTY_V8_ARCHIVE, if the caller set it: KREBS_V8_FEATURES says whether that library has the cage per isolate.
#   3. Nothing: the v8 crate downloads its librusty_v8_release_<target>.a.gz from denoland/rusty_v8 once and keeps it under
#      CARGO_TARGET_DIR. No pointer compression, so a VM takes more memory and a tick a little longer than on Windows.
_krebs_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
_krebs_fast="$_krebs_root/target/ptrcomp/release/gn_out"
if [ -z "${RUSTY_V8_ARCHIVE:-}" ] && [ -f "$_krebs_fast/obj/librusty_v8.a" ]; then
    export RUSTY_V8_ARCHIVE="$_krebs_fast/obj/librusty_v8.a"
    export RUSTY_V8_SRC_BINDING_PATH="$_krebs_fast/src_binding.rs"
    KREBS_V8_FEATURES="${KREBS_V8_FEATURES---features runtime/ptrcomp}"
fi
KREBS_V8_FEATURES="${KREBS_V8_FEATURES-}"
unset _krebs_root _krebs_fast
