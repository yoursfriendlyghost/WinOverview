@echo off
setlocal
title Uninstall WinOverview

echo.
echo  This removes WinOverview and everything it saved on this PC:
echo    - its settings          %APPDATA%\WinOverview
echo    - "Start with Windows"
echo    - WinOverview.exe in this folder, if it's here
echo.
choice /c YN /m " Uninstall WinOverview"
if errorlevel 2 exit /b

echo.
taskkill /im WinOverview.exe /f >nul 2>&1
timeout /t 1 /nobreak >nul
reg delete "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v WinOverview /f >nul 2>&1
rmdir /s /q "%APPDATA%\WinOverview" >nul 2>&1
rmdir /s /q "%LOCALAPPDATA%\WinOverview" >nul 2>&1
if exist "%~dp0WinOverview.exe" del /q "%~dp0WinOverview.exe" >nul 2>&1

echo  WinOverview has been uninstalled. You can delete this file now.
echo.
pause
