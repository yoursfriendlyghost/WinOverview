@echo off
setlocal
title Uninstall WinOverview

echo.
echo  This removes WinOverview and everything it saved on this PC:
echo    - its settings              %APPDATA%\WinOverview
echo    - its helper files          %LOCALAPPDATA%\WinOverview
echo    - unpacked runtime files    %TEMP%\.net\WinOverview
echo    - "Start with Windows"
echo    - WinOverview.exe in this folder, if it's here
echo.
choice /c YN /m " Uninstall WinOverview"
if errorlevel 2 exit /b

echo.
echo  Closing WinOverview...
taskkill /im WinOverview.exe /f >nul 2>&1
timeout /t 2 /nobreak >nul

echo  Removing "Start with Windows"...
reg delete "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v WinOverview /f >nul 2>&1
del /q "%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\WinOverview.lnk" >nul 2>&1

echo  Removing settings and helper files...
rmdir /s /q "%APPDATA%\WinOverview" >nul 2>&1
rmdir /s /q "%LOCALAPPDATA%\WinOverview" >nul 2>&1
rmdir /s /q "%TEMP%\.net\WinOverview" >nul 2>&1

set "TASKBAR_NOTE="
if exist "%TEMP%\WinOverview.autohide" (
    del /q "%TEMP%\WinOverview.autohide" >nul 2>&1
    set "TASKBAR_NOTE=1"
)

if exist "%~dp0WinOverview.exe" (
    echo  Deleting WinOverview.exe...
    del /q "%~dp0WinOverview.exe" >nul 2>&1
)

echo.
echo  WinOverview has been uninstalled.
if defined TASKBAR_NOTE (
    echo.
    echo  Note: WinOverview had switched your taskbar's auto-hide off when it last closed.
    echo  To turn it back on: Settings ^> Personalization ^> Taskbar ^> Taskbar behaviors.
)
echo  You can delete this file and the WinOverview folder now.
echo.
pause
