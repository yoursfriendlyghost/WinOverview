@echo off
setlocal
cd /d "%~dp0"
where cargo >nul 2>&1
if errorlevel 1 (
    echo Rust isn't installed. Install it with:
    echo     winget install Rustlang.Rustup
    echo and the "Desktop development with C++" part of Visual Studio Build Tools:
    echo     winget install Microsoft.VisualStudio.2022.BuildTools
    pause
    exit /b 1
)
if not exist dist mkdir dist
echo Building WinOverview (the first build takes a few minutes)...

rem Smallest build (same as GitHub's): also rebuilds Rust's standard library tuned for size.
rem Needs Rust's source: rustup component add rust-src. Falls back to a normal build.
set RUSTC_BOOTSTRAP=1
rustup component list --installed 2>nul | findstr /b "rust-src" >nul
if errorlevel 1 goto normal
cargo build --release --target x86_64-pc-windows-msvc -Z build-std=std,panic_abort -Z build-std-features=optimize_for_size
if errorlevel 1 goto normal
copy /y target\x86_64-pc-windows-msvc\release\winoverview.exe dist\WinOverview.exe >nul
goto packed

:normal
echo (Normal build. For a smaller exe run: rustup component add rust-src)
cargo build --release
if errorlevel 1 ( pause & exit /b 1 )
copy /y target\release\winoverview.exe dist\WinOverview.exe >nul

:packed
copy /y uninstall.bat dist\uninstall.bat >nul

rem Squeezed copy with UPX, if upx.exe is installed (winget install UPX.UPX) or next to this file
set UPX=
where upx >nul 2>&1 && set UPX=upx
if exist upx.exe set UPX=upx.exe
if defined UPX (
    copy /y dist\WinOverview.exe dist\WinOverview-small.exe >nul
    %UPX% --best --lzma -q dist\WinOverview-small.exe >nul
)

echo.
for %%f in (dist\*.exe) do echo Done: %%f  (%%~zf bytes)
pause
