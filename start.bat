@echo off
rem Double-click to build (if needed) and launch WinOverview.
rem "Start with Windows" is a checkbox in the tray icon's menu.
rem   start.bat remove   -> delete the old startup shortcut from earlier versions
if /i "%1"=="remove" (
    powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0start.ps1" -Remove
) else (
    powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0start.ps1"
)
timeout /t 2 >nul
