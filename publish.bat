@echo off
rem Double-click to build the one-file release: dist\WinOverview.exe
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0publish.ps1"
pause
