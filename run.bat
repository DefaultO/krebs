@echo off
setlocal
rem For developers: rebuilds Krebs if the code changed, creates the world on first run, starts the server.
rem Players do not need this: krebs.exe starts on a double-click.
rem Extra arguments go to the server, for example: run.bat --tick-rate 0
cd /d "%~dp0"

where cargo >nul 2>nul
if errorlevel 1 (
    echo Rust was not found, so Krebs cannot be built.
    echo Get Rust from https://rustup.rs
    echo Its setup also offers the C++ build tools it needs. If it does not, get them from
    echo https://visualstudio.microsoft.com/visual-cpp-build-tools/
    echo Then open a new window and run this again.
    pause
    exit /b 1
)

rem With mods here, a build lists them in both files, and neither list belongs in a commit. README.md says how to undo this.
if exist .git if exist mods\bare\Cargo.toml (
    git -c safe.directory=* update-index --skip-worktree crates/modlist/Cargo.toml >nul 2>nul
    git -c safe.directory=* update-index --skip-worktree Cargo.lock >nul 2>nul
)

rem The fast V8 from tools/v8-ptrcomp.ps1 when it has been built here, otherwise the same library downloaded once.
set "FAST=%~dp0target\ptrcomp\release\gn_out"
set "FEATURES=--features runtime/ptrcomp"
set "CARGO_TARGET_DIR=%~dp0target\fast"
set "EXE=target\fast\release\krebs.exe"
if exist "%FAST%\obj\rusty_v8.lib" (
    set "RUSTY_V8_ARCHIVE=%FAST%\obj\rusty_v8.lib"
    set "RUSTY_V8_SRC_BINDING_PATH=%FAST%\src_binding.rs"
) else (
    set "RUSTY_V8_ARCHIVE=https://gitea.defaul.to/DefaultO/krebs/releases/download/v8-152.2.0-fast/rusty_v8.lib.gz"
    set "RUSTY_V8_ARCHIVE_SHA256=104c007dafbc047dbd048ec4ae9c9e2b14feda2309c97f014a3973c8ea1c1812"
)

rem Page files are read from here on every request, so a reconnect shows an edit.
set "KREBS_PAGES_DIR=%~dp0mods\bare"
set "MODS="
if exist mods\bare\Cargo.toml set "MODS=--mods bare"

rem The first build after the mods folder changes stops on purpose (crates\modlist\build.rs), and the second compiles them in.
cargo build --release -p server %FEATURES% -j 4
if errorlevel 1 cargo build --release -p server %FEATURES% -j 4
if errorlevel 1 (
    echo Build failed, server not started.
    pause
    exit /b 1
)

if not exist data\world.json %EXE% new --world 2x2 --seed 1 --data data

echo.
echo Connect the Screeps client to 127.0.0.1, port 21025. Press Ctrl+C to stop and save.
%EXE% serve --data data %MODS% %*
pause
