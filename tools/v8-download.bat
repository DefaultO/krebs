@echo off
rem Gets the fast V8 library and points the build at it. run.bat and tools\dist.bat call this, with CARGO_TARGET_DIR set.
rem The same file is a release on GitHub and on Gitea, so that either site can be gone. RUSTY_V8_ARCHIVE can only name one
rem address, so the file is fetched here and the build reads it from disk. It stays in CARGO_TARGET_DIR, so it downloads once.
rem The sha256 pins it, whichever site it came from, and the build checks it again. A new library means a new tag and sha256 here.
set "V8_TAG=v8-152.2.0-fast"
set "V8_SHA256=104c007dafbc047dbd048ec4ae9c9e2b14feda2309c97f014a3973c8ea1c1812"
set "V8_PATH=DefaultO/krebs/releases/download/%V8_TAG%/rusty_v8.lib.gz"
set "V8_DIR=%CARGO_TARGET_DIR%\v8-download"
set "V8_FILE=%V8_DIR%\rusty_v8.lib.gz"

call :matches "%V8_FILE%" && goto found
if not exist "%V8_DIR%" mkdir "%V8_DIR%"
call :fetch "https://github.com/%V8_PATH%" && goto found
call :fetch "https://gitea.defaul.to/%V8_PATH%" && goto found

echo.
echo Could not download the fast V8 library from GitHub or from Gitea.
echo Check your internet connection and try again. Or save the file in a browser, from either of these:
echo   https://github.com/%V8_PATH%
echo   https://gitea.defaul.to/%V8_PATH%
echo as "%V8_FILE%"
exit /b 1

:found
set "RUSTY_V8_ARCHIVE=%V8_FILE%"
set "RUSTY_V8_ARCHIVE_SHA256=%V8_SHA256%"
exit /b 0

rem A wrong page with a good status counts as a failed site too, so the next one gets its turn.
:fetch
echo Downloading the fast V8 library from %~1
"%SystemRoot%\System32\curl.exe" --fail --location --progress-bar --connect-timeout 20 --speed-time 60 --speed-limit 1024 --output "%V8_FILE%.part" "%~1"
if errorlevel 1 (
    if exist "%V8_FILE%.part" del /q "%V8_FILE%.part"
    echo That failed.
    exit /b 1
)
call :matches "%V8_FILE%.part" && goto keep
del /q "%V8_FILE%.part"
echo That is not the expected file: its sha256 is wrong.
exit /b 1
:keep
move /y "%V8_FILE%.part" "%V8_FILE%" >nul
exit /b

:matches
if not exist "%~1" exit /b 1
certutil -hashfile "%~1" SHA256 | findstr /i /x "%V8_SHA256%" >nul
exit /b
