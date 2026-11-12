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
echo Building WinOverview (the first build takes a few minutes)...
cargo build --release
if errorlevel 1 ( pause & exit /b 1 )
if not exist dist mkdir dist
copy /y target\release\winoverview.exe dist\WinOverview.exe >nul
copy /y uninstall.bat dist\uninstall.bat >nul
echo.
echo Done: dist\WinOverview.exe
pause
