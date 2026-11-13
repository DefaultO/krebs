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

# 3 pins what it fetches, as tools/v8-download.bat pins the library for Windows: the build stops if the archive is not
# the one with this sha256. These are the digests GitHub lists for the archives of denoland/rusty_v8 v152.2.0, the
# version in vendor/v8, and they change with it. A toolchain for another target, or a debug V8, fetches another file
# and gets no pin.
if [ -z "${RUSTY_V8_ARCHIVE:-}" ] && [ -z "${RUSTY_V8_ARCHIVE_SHA256:-}" ] && [ -z "${V8_FORCE_DEBUG:-}" ]; then
    _krebs_host="$(rustc -vV 2>/dev/null | sed -n 's/^host: //p' || true)"
    _krebs_pin=""
    case "$_krebs_host" in
        x86_64-unknown-linux-gnu) _krebs_pin=b6683e9afcb77fbd8cb2c3acf45d34d52029ab216b332522c95179c12f0fed3c ;;
        aarch64-unknown-linux-gnu) _krebs_pin=c40196bbf182532e583f65b7daf0196fdbb3354e6fac59b31d0539c20f931f3d ;;
        aarch64-apple-darwin) _krebs_pin=a4a5484444ad4b5f10dc7bed141236dd9dbeabf2006d14520f19b935d4db4227 ;;
        x86_64-apple-darwin) _krebs_pin=04cbc1c36c4cd5459c54049775d92889337c69d936fb0b011f9b26f1d454610d ;;
    esac
    if [ -n "$_krebs_pin" ]; then
        export RUSTY_V8_ARCHIVE_SHA256="$_krebs_pin"
    fi
    unset _krebs_host _krebs_pin
fi
unset _krebs_root _krebs_fast
