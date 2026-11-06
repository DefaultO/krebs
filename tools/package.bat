@echo off
setlocal
rem Builds the download for players and leaves it in dist\.
rem It builds in a clean copy of this folder: only what a clone with its submodules has, so no private mod, world or old
rem build gets in, and the mod list in crates\modlist\Cargo.toml stays as it is here. Nothing else in this folder changes.
rem The build goes to CARGO_TARGET_DIR if that is set, otherwise to target\package. The fast V8 comes from
rem RUSTY_V8_ARCHIVE if that is set, then from this PC's own build, then from the download tools\dist.bat knows.
rem The files that go beside the program are in tools\package\.
cd /d "%~dp0.."
set "ROOT=%CD%"

where cargo >nul 2>nul
if errorlevel 1 (
    echo Rust was not found. Get it from https://rustup.rs
    exit /b 1
)
for %%m in (bare matchrun) do if not exist mods\%%m\Cargo.toml (
    echo mods\%%m is missing, and players cannot have a match without it. Run: git submodule update --init
    exit /b 1
)

for /f "tokens=3" %%v in ('findstr /b /c:"version = " Cargo.toml') do set "VERSION=%%~v"
if not defined VERSION (
    echo No version found in Cargo.toml.
    exit /b 1
)
set "ZIP=%ROOT%\dist\Krebs-v%VERSION%-windows-x64.zip"

if not defined CARGO_TARGET_DIR set "CARGO_TARGET_DIR=%ROOT%\target\package"
for %%d in ("%CARGO_TARGET_DIR%") do set "CARGO_TARGET_DIR=%%~fd"
set "SOURCE=%CARGO_TARGET_DIR%\source"
set "STAGE=%CARGO_TARGET_DIR%\stage"

set "FAST=%ROOT%\target\ptrcomp\release\gn_out"
if not defined RUSTY_V8_ARCHIVE if exist "%FAST%\obj\rusty_v8.lib" (
    set "RUSTY_V8_ARCHIVE=%FAST%\obj\rusty_v8.lib"
    set "RUSTY_V8_SRC_BINDING_PATH=%FAST%\src_binding.rs"
)

echo Cleaning.
if exist "%SOURCE%" rmdir /s /q "%SOURCE%"
if exist "%STAGE%" rmdir /s /q "%STAGE%"
if exist "%CARGO_TARGET_DIR%\dist\krebs.exe" del /q "%CARGO_TARGET_DIR%\dist\krebs.exe"
mkdir "%STAGE%"

echo Copying the source.
robocopy "%ROOT%" "%SOURCE%" /E /XJ /NFL /NDL /NJH /NJS /NP /XF .git /XD "%ROOT%\.git" "%ROOT%\.claude" "%ROOT%\target" "%ROOT%\data" "%ROOT%\reference" "%ROOT%\dist" "%ROOT%\mods\platform" "%ROOT%\vendor\v8\build" "%ROOT%\vendor\v8\buildtools" "%ROOT%\vendor\v8\third_party" "%ROOT%\vendor\v8\tools" "%ROOT%\vendor\v8\v8" >nul
if errorlevel 8 (
    echo Copying the source failed.
    exit /b 1
)
rem run.bat leaves the mods of this PC listed here, and a listed mod that was not copied would stop Cargo before build.rs runs.
set "LIST=%SOURCE%\crates\modlist\Cargo.toml"
findstr /v /c:"../../mods/" "%LIST%" > "%LIST%.new"
move /y "%LIST%.new" "%LIST%" >nul

echo Building. A first build takes several minutes.
call "%SOURCE%\tools\dist.bat"
if errorlevel 1 exit /b 1

copy /y "%CARGO_TARGET_DIR%\dist\krebs.exe" "%STAGE%\Krebs.exe" >nul
if errorlevel 1 (
    echo The build left no krebs.exe.
    exit /b 1
)

rem The notices name the crates in Rust's standard library by pointing at this file.
for /f "delims=" %%s in ('rustc --print sysroot') do set "SYSROOT=%%s"
copy /y "%SYSROOT%\share\doc\rust\COPYRIGHT-library.html" "%STAGE%\RUST_NOTICES.html" >nul
if errorlevel 1 (
    echo The Rust toolchain has no COPYRIGHT-library.html, so it looks damaged. Reinstall it with rustup.
    exit /b 1
)

copy /y "%ROOT%\tools\package\krebs.json" "%STAGE%\" >nul
copy /y "%ROOT%\tools\package\Let friends join.bat" "%STAGE%\" >nul
mkdir "%STAGE%\starter-bot"
copy /y "%ROOT%\tools\package\starter-bot\main.js" "%STAGE%\starter-bot\" >nul
if errorlevel 1 (
    echo Could not copy the starter bot.
    exit /b 1
)
call :text "%ROOT%\tools\package\README.txt" "%STAGE%\README.txt"
if errorlevel 1 exit /b 1
call :text "%ROOT%\LICENSE" "%STAGE%\LICENSE.txt"
if errorlevel 1 exit /b 1
call :text "%ROOT%\THIRD_PARTY_NOTICES.md" "%STAGE%\THIRD_PARTY_NOTICES.txt"
if errorlevel 1 exit /b 1

rem The program has to start, has to run on a PC without Visual Studio's DLLs, and has to hold what a match needs.
"%STAGE%\Krebs.exe" --help >nul
if errorlevel 1 (
    echo The new Krebs.exe does not start.
    exit /b 1
)
for %%d in (VCRUNTIME140 MSVCP140) do (
    findstr /m /c:"%%d" "%STAGE%\Krebs.exe" >nul
    if not errorlevel 1 (
        echo The new Krebs.exe needs %%d.dll, which a player may not have. See the link arguments in tools\dist.bat.
        exit /b 1
    )
)
for %%s in ("/api/lobbies" "/api/matchrun/view") do (
    findstr /m /c:%%s "%STAGE%\Krebs.exe" >nul
    if errorlevel 1 (
        echo The new Krebs.exe does not have %%s, so a mod is not in it.
        exit /b 1
    )
)

rem Only now: a build that stops on the way leaves the last zip where it was.
if exist "%ROOT%\dist" rmdir /s /q "%ROOT%\dist"
mkdir "%ROOT%\dist"
"%SystemRoot%\System32\tar.exe" -a -c -f "%ZIP%" -C "%STAGE%" Krebs.exe krebs.json "Let friends join.bat" README.txt LICENSE.txt THIRD_PARTY_NOTICES.txt RUST_NOTICES.html starter-bot
if errorlevel 1 (
    echo Making the zip failed.
    exit /b 1
)
"%SystemRoot%\System32\tar.exe" -t -v -f "%ZIP%"
for %%z in ("%ZIP%") do echo Built %%~fz, %%~zz bytes
exit /b 0

rem Notepad wants Windows line endings, whatever line endings a checkout has.
:text
set "FROM=%~1"
set "TO=%~2"
powershell -NoProfile -NonInteractive -Command "$t = [IO.File]::ReadAllText($env:FROM) -replace '\r?\n', [Environment]::NewLine; [IO.File]::WriteAllText($env:TO, $t, [Text.Encoding]::UTF8)"
if errorlevel 1 (
    echo Could not write "%TO%"
    exit /b 1
)
exit /b 0
