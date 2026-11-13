@echo off
setlocal
rem For developers on Windows: checks that Krebs still compiles for Linux and macOS. A change that only builds on Windows
rem goes unnoticed here, and has: a call into a Windows-only module stopped every Linux build until someone on Linux said so.
rem
rem It type-checks every crate and every test for the four targets below. Nothing is linked or run, so this finds what
rem does not compile there, not what goes wrong when it runs. No V8 library is fetched: the bindings for each target come
rem with vendor\v8, and a check links nothing. The first run takes a few minutes and leaves about 2 GB in
rem target\check-platforms, or under CARGO_TARGET_DIR if that is set. Later runs take seconds.
rem
rem The targets' standard libraries have to be there, which is one download from rust-lang.org each:
rem   rustup target add x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu aarch64-apple-darwin x86_64-apple-darwin
cd /d "%~dp0.."

where cargo >nul 2>nul
if errorlevel 1 (
    echo Rust was not found. Get it from https://rustup.rs
    exit /b 1
)

set "TARGETS=x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu aarch64-apple-darwin x86_64-apple-darwin"
set "MISSING="
for %%t in (%TARGETS%) do (
    rem rustup ends its lines without a carriage return, which findstr /x does not take for a whole line
    rustup target list --installed | findstr /b /c:"%%t" >nul
    if errorlevel 1 call set "MISSING=%%MISSING%% %%t"
)
if defined MISSING (
    echo These targets are not installed:%MISSING%
    echo Install them once, then run this again:
    echo   rustup target add%MISSING%
    exit /b 1
)

rem Whatever V8 library this window was set up for belongs to Windows.
set "RUSTY_V8_ARCHIVE="
set "RUSTY_V8_ARCHIVE_SHA256="
set "RUSTY_V8_SRC_BINDING_PATH="
set "RUSTY_V8_SKIP_DOWNLOAD=1"
if not defined CARGO_TARGET_DIR set "CARGO_TARGET_DIR=%CD%\target\check-platforms"

for %%t in (%TARGETS%) do (
    echo.
    echo Checking %%t
    rem The first build after the mods folder changes stops on purpose ^(crates\modlist\build.rs^), and the second takes them in.
    cargo check --workspace --tests --target %%t -j 4
    if errorlevel 1 cargo check --workspace --tests --target %%t -j 4
    if errorlevel 1 (
        echo.
        echo Krebs does not compile for %%t.
        exit /b 1
    )
)
echo.
echo Krebs compiles for Linux and macOS, on x64 and arm64.
