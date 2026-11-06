# Builds chitin on V8 with pointer compression and a cage per isolate (docs/ARCHITECTURE.md 2.9), compiled from source:
# ~40 min at -j 4 the first time, plus ~3 GB of Chromium toolchains the v8 build script downloads.
#   powershell -ExecutionPolicy Bypass -File tools\v8-ptrcomp.ps1 [cargo args]   # default: build --release -j 4 -p server
# Cargo runs with --features runtime/ptrcomp and its own target dir, target\ptrcomp, so the library built here never
# replaces the prebuilt one in target\release. The result is target\ptrcomp\release\chitin.exe. V8's builtins are
# laid out by the profile in v8-ptrcomp.profile (builtins PGO); v8-ptrcomp.patch holds chitin's changes to V8's source.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$v8 = [IO.Path]::GetFullPath("$PSScriptRoot\..\vendor\v8")
$tar = "$env:SystemRoot\System32\tar.exe"  # bsdtar: Git's GNU tar takes E:\... for a remote host

function Fetch($url, $out) {
    curl.exe -fsSL --retry 3 -o $out $url
    if ($LASTEXITCODE) { throw "download failed: $url" }
}
function Check($what, $actual, $expected) {
    if ($actual -ne $expected) { throw "$what`: sha256 $actual, expected $expected" }
}
function Sha256($bytes) { -join ([Security.Cryptography.SHA256]::Create().ComputeHash($bytes) | ForEach-Object { $_.ToString('x2') }) }
# A tar from gitiles stamps every entry with the download time, so the files are checked, not the archive.
function TreeDigest($dir) {
    $n = (Get-Item $dir).FullName.Length + 1
    [string[]]$lines = Get-ChildItem $dir -Recurse -File -Force | ForEach-Object {
        (Get-FileHash -Algorithm SHA256 $_.FullName).Hash.ToLower() + ' ' + $_.FullName.Substring($n).Replace('\', '/')
    }
    [Array]::Sort($lines, [StringComparer]::Ordinal)
    Sha256 ([Text.Encoding]::UTF8.GetBytes($lines -join "`n"))
}

$patch = "$PSScriptRoot\v8-ptrcomp.patch"
$stamp = "$v8\third_party\.chitin-sources"  # the patch's hash: a changed patch extracts the sources again
$patched = (Get-FileHash -Algorithm SHA256 $patch).Hash.ToLower()
if (-not (Test-Path $stamp) -or (Get-Content $stamp) -ne $patched) {
    $tmp = "$v8\third_party\.chitin-tmp"  # on vendor's drive: Move-Item cannot move a directory across drives
    if (Test-Path $tmp) { Remove-Item -Recurse -Force $tmp }
    New-Item -ItemType Directory $tmp | Out-Null

    # The V8 source tree vendor/v8 leaves out, from the crate Cargo.lock pinned before the patch.
    Fetch 'https://static.crates.io/crates/v8/v8-152.2.0.crate' "$tmp\v8.crate"
    Check 'v8-152.2.0.crate' (Get-FileHash -Algorithm SHA256 "$tmp\v8.crate").Hash.ToLower() 'a10fe1a92da5c32c7c7f838ce36c0ccfcfd5edf0865b58bdde820aa64cea9888'
    # -m: the crate's files are dated 2006, which would hide what an earlier patch changed from ninja
    & $tar -xzmf "$tmp\v8.crate" -C $v8 --strip-components=1 v8-152.2.0/build v8-152.2.0/buildtools v8-152.2.0/third_party v8-152.2.0/tools v8-152.2.0/v8
    if ($LASTEXITCODE) { throw 'tar failed: v8.crate' }
    git -C "$PSScriptRoot\.." apply --directory=vendor/v8 $patch
    if ($LASTEXITCODE) { throw 'git apply failed: v8-ptrcomp.patch' }

    # What crates.io left out of the crate, at the commits v8/DEPS pins. ICU's data is part of what players see (Intl).
    Fetch 'https://chromium.googlesource.com/chromium/deps/icu.git/+/d578f2e8b7bd5938e21cfb6bf15c079e0aa5b738/common/icudtl.dat?format=TEXT' "$tmp\icudtl.b64"
    $icu = [Convert]::FromBase64String([IO.File]::ReadAllText("$tmp\icudtl.b64"))
    Check 'icudtl.dat' (Sha256 $icu) '9f48c7f9c7c94d516a14870707e910ab94d75ae640ff6842c4af53276cd26ebe'
    New-Item -ItemType Directory -Force "$v8\third_party\icu\common" | Out-Null
    [IO.File]::WriteAllBytes("$v8\third_party\icu\common\icudtl.dat", $icu)

    # Every nested Cargo package under chromium_crates_io (crates.io drops directories holding a Cargo.toml).
    Fetch 'https://chromium.googlesource.com/chromium/src/third_party/rust/+archive/afbc96607d0e659715d803cc099607dd1737fc41/chromium_crates_io.tar.gz' "$tmp\crates.tar.gz"
    New-Item -ItemType Directory "$tmp\chromium_crates_io" | Out-Null
    & $tar -xzf "$tmp\crates.tar.gz" -C "$tmp\chromium_crates_io"
    if ($LASTEXITCODE) { throw 'tar failed: chromium_crates_io' }
    Check 'chromium_crates_io' (TreeDigest "$tmp\chromium_crates_io") '8506cf30d7d17af2224772cf10ed113d03211dfa842c13a4981842a9e7dc5408'
    if (Test-Path "$v8\third_party\rust\chromium_crates_io") { Remove-Item -Recurse -Force "$v8\third_party\rust\chromium_crates_io" }
    Move-Item "$tmp\chromium_crates_io" "$v8\third_party\rust\chromium_crates_io"

    Remove-Item -Recurse -Force $tmp
    Set-Content -Path $stamp -Value $patched -NoNewline
}

# Builtins PGO (docs/ARCHITECTURE.md 2.9): mksnapshot takes branch hints and block order from the counts recorded on the
# grown world. The profile's hashes tie it to this V8, these GN args and Windows clang. Builtin reordering stays off
# (mksnapshot writes Windows unwind data in id order: LNK1223). Left out while recording a new profile.
if ($env:GN_ARGS -notmatch 'v8_enable_builtins_profiling') {
    $hints = "$PSScriptRoot\v8-ptrcomp.profile" -replace '\\', '/'
    $env:GN_ARGS = "$env:GN_ARGS v8_builtins_profiling_log_file=`"$hints`" v8_enable_builtins_reordering=false"
}

$env:V8_FROM_SOURCE = '1'
Remove-Item Env:RUSTY_V8_ARCHIVE -ErrorAction SilentlyContinue
# V8's ICU step fails under the Store's python3.exe alias, so the build gets a real interpreter.
if (-not $env:PYTHON) { $env:PYTHON = py -3 -c 'import sys; print(sys.executable)' }
$env:CARGO_TARGET_DIR = [IO.Path]::GetFullPath("$PSScriptRoot\..\target\ptrcomp")
$sub, $rest = if ($args.Count) { $args } else { 'build', '--release', '-j', '4', '-p', 'server' }
Push-Location "$PSScriptRoot\.."
try {
    cargo $sub --features runtime/ptrcomp @rest
    exit $LASTEXITCODE
} finally {
    Pop-Location
}
