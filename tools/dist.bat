@echo off
setlocal
rem Builds the krebs.exe that goes to players, with every mod that is checked out in mods\. It ends up in dist\ under
rem CARGO_TARGET_DIR, or in target\dist-exe\dist\ if that is not set. tools\package.bat calls this and makes the zip around it.
rem No path of this machine may be in it. The dist profile drops the debug information, and the remaps below cover what is left:
rem crates from the registry name their own folder in panic messages. They go through --config, as RUSTFLAGS splits on spaces.
cd /d "%~dp0.."

if exist mods\platform\Cargo.toml (
    echo mods\platform is private, and every mod found here gets built in. Use tools\package.bat, which leaves it out.
    exit /b 1
)
where cargo >nul 2>nul
if errorlevel 1 (
    echo Rust was not found. Get it from https://rustup.rs
    exit /b 1
)

set "FAST=%CD%\target\ptrcomp\release\gn_out"
if not defined CARGO_TARGET_DIR set "CARGO_TARGET_DIR=%CD%\target\dist-exe"
if defined RUSTY_V8_ARCHIVE (
    rem The caller chose the library.
) else if exist "%FAST%\obj\rusty_v8.lib" (
    set "RUSTY_V8_ARCHIVE=%FAST%\obj\rusty_v8.lib"
    set "RUSTY_V8_SRC_BINDING_PATH=%FAST%\src_binding.rs"
) else (
    call tools\v8-download.bat
    if errorlevel 1 exit /b 1
)
set "CARGO_FOLDER=%CARGO_HOME%"
if not defined CARGO_FOLDER set "CARGO_FOLDER=%USERPROFILE%\.cargo"
rem Single quotes keep backslashes and spaces as they are, and a folder with one in its name would end them.
set "FOLDERS=%CARGO_FOLDER%%CARGO_TARGET_DIR%%CD%"
if not "%FOLDERS:'=%"=="%FOLDERS%" (
    echo Krebs cannot be built in a folder with an apostrophe in its name. Rename the folder or move Krebs, then run this again.
    exit /b 1
)
rem The two link arguments build Microsoft's vcruntime into the exe, so that it needs no DLL a player might not have.
set "REMAPS=build.rustflags=['--remap-path-prefix=%CARGO_FOLDER%=cargo','--remap-path-prefix=%CARGO_TARGET_DIR%=target','--remap-path-prefix=%CD%=krebs','-Clink-arg=/NODEFAULTLIB:vcruntime.lib','-Clink-arg=/DEFAULTLIB:libvcruntime.lib']"
rem Either of these would win over the remaps.
set "RUSTFLAGS="
set "CARGO_ENCODED_RUSTFLAGS="

rem The first build after the mods folder changes stops on purpose (crates\modlist\build.rs), and the second compiles them in.
cargo build --config "%REMAPS%" --profile dist -p server --features runtime/ptrcomp -j 4
if errorlevel 1 cargo build --config "%REMAPS%" --profile dist -p server --features runtime/ptrcomp -j 4
if errorlevel 1 (
    echo Build failed.
    exit /b 1
)
echo Built "%CARGO_TARGET_DIR%\dist\krebs.exe"
