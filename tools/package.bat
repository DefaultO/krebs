@echo off
setlocal
rem Builds the download for players and leaves it in dist\.
rem A release is built from a fresh clone at its tag, so that it holds what that commit says and nothing else. So this stops
rem at what a clone would not have: uncommitted changes, changes git status hides, a submodule that differs from its link,
rem or a mod folder that is not in the repository. KREBS_ALLOW_DIRTY=1 lets a trial build through, which is not for publishing.
rem It then builds in a copy of this folder with no world, old build or mods\platform, which is private, and the mod list in
rem crates\modlist\Cargo.toml stays as it is here. Nothing else in this folder changes. Files git ignores may be copied, and
rem the build reads none of them.
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
if "%KREBS_ALLOW_DIRTY%"=="1" (
    echo KREBS_ALLOW_DIRTY is set, so this build may hold what no commit has. Do not publish it.
) else (
    call :committed
    if errorlevel 1 exit /b 1
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
set "STAGE_ROOT=%CARGO_TARGET_DIR%\stage"
rem Everything in the zip sits in one folder, so that Extract Here in Downloads does not leave Krebs.exe among the downloads.
set "STAGE=%STAGE_ROOT%\Krebs"

set "FAST=%ROOT%\target\ptrcomp\release\gn_out"
if not defined RUSTY_V8_ARCHIVE if exist "%FAST%\obj\rusty_v8.lib" (
    set "RUSTY_V8_ARCHIVE=%FAST%\obj\rusty_v8.lib"
    set "RUSTY_V8_SRC_BINDING_PATH=%FAST%\src_binding.rs"
)

echo Cleaning.
if exist "%SOURCE%" rmdir /s /q "%SOURCE%"
if exist "%STAGE_ROOT%" rmdir /s /q "%STAGE_ROOT%"
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
"%SystemRoot%\System32\tar.exe" -a -c -f "%ZIP%" -C "%STAGE_ROOT%" Krebs
if errorlevel 1 (
    echo Making the zip failed.
    exit /b 1
)
"%SystemRoot%\System32\tar.exe" -t -v -f "%ZIP%"
for %%z in ("%ZIP%") do echo Built %%~fz, %%~zz bytes
exit /b 0

rem Stops unless this folder is one commit and the commits it links to, and lists what is not.
:committed
where git >nul 2>nul
if errorlevel 1 (
    echo git was not found, so nothing here can say what is in this folder. For a trial build, set KREBS_ALLOW_DIRTY=1.
    exit /b 1
)
set "GIT=git -c safe.directory=*"
set "FOUND=%TEMP%\krebs-package-%RANDOM%%RANDOM%.txt"
%GIT% rev-parse --verify HEAD >nul 2>nul
if errorlevel 1 (
    echo This folder is not a git clone, so nothing can say what is in it. A release is built from a fresh clone at its tag.
    exit /b 1
)
rem Changed and new files, here and in the submodules, and a submodule on another commit.
%GIT% status --porcelain --ignore-submodules=none > "%FOUND%"
if errorlevel 1 goto :git_failed
call :none_of "Uncommitted changes"
if errorlevel 1 exit /b 1
rem A submodule status starts with a space when it matches its link.
%GIT% submodule status --recursive > "%FOUND%.all"
if errorlevel 1 goto :git_failed
findstr /v /b /c:" " "%FOUND%.all" > "%FOUND%"
del "%FOUND%.all"
call :none_of "Submodules that differ from their links"
if errorlevel 1 exit /b 1
rem git status cannot see into these: run.bat marks Cargo.lock and the mod list this way.
%GIT% ls-files -v > "%FOUND%.all"
if errorlevel 1 goto :git_failed
findstr /b /c:"S " /c:"h " "%FOUND%.all" > "%FOUND%"
del "%FOUND%.all"
call :none_of "Files git status has been told to skip"
if errorlevel 1 exit /b 1
rem crates\modlist\build.rs compiles in every folder here. mods\platform stays out of the copy, so it cannot get in.
(for /f "delims=" %%m in ('dir /b /ad mods') do if /i not "%%m"=="platform" (
    %GIT% ls-tree --name-only HEAD "mods/%%m" | findstr /r "." >nul || echo mods\%%m
)) > "%FOUND%"
call :none_of "Mod folders that are not in the repository"
if errorlevel 1 exit /b 1
del "%FOUND%"
exit /b 0

:git_failed
echo git could not say what is in this folder.
if exist "%FOUND%" del "%FOUND%"
if exist "%FOUND%.all" del "%FOUND%.all"
exit /b 1

:none_of
for %%f in ("%FOUND%") do if %%~zf gtr 0 (
    echo %~1, so a build here would hold what no commit says:
    type "%FOUND%"
    echo A release is built from a fresh clone at its tag. For a trial build, set KREBS_ALLOW_DIRTY=1.
    del "%FOUND%"
    exit /b 1
)
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
