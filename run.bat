@echo off
cd /d "%~dp0"
taskkill /im winoverview.exe /f >nul 2>&1
cargo run --release
