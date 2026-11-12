# Builds WinOverview (only when the code has changed) and launches it in the background.
# "Start with Windows" is now a checkbox in the tray icon's menu.
#   .\start.ps1           build if needed + run
#   .\start.ps1 -Remove   delete the old startup shortcut made by earlier versions of this script

param([switch]$Remove)

$ErrorActionPreference = "Stop"
$root = $PSScriptRoot
$out  = Join-Path $root "bin\app"
$exe  = Join-Path $out "WinOverview.exe"
$oldLink = Join-Path ([Environment]::GetFolderPath("Startup")) "WinOverview.lnk"

if ($Remove) {
    if (Test-Path $oldLink) { Remove-Item $oldLink; Write-Host "Removed the old startup shortcut." }
    else { Write-Host "No old startup shortcut found." }
    exit
}

# Stop a running copy so the build can overwrite the .exe.
# Ask nicely first (lets it restore the taskbar and remove its tray icon), then force.
if (Get-Process WinOverview -ErrorAction SilentlyContinue) {
    Write-Host "Closing the running copy..."
    taskkill /im WinOverview.exe 2>&1 | Out-Null
    Get-Process WinOverview -ErrorAction SilentlyContinue | Wait-Process -Timeout 3 -ErrorAction SilentlyContinue
    Get-Process WinOverview -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 500

    if (Get-Process WinOverview -ErrorAction SilentlyContinue) {
        Write-Host "Couldn't close the running WinOverview." -ForegroundColor Red
        Write-Host "Quit it from its tray icon (or end it in Task Manager), then run this again."
        Read-Host "Press Enter"
        exit 1
    }
}

# Rebuild only if any source file is newer than the last build
$sources = Get-ChildItem $root -File -Recurse -Include *.cs, *.xaml, *.csproj, *.manifest, *.ico, *.dll |
           Where-Object { $_.FullName -notmatch '\\(bin|obj|dist)\\' }
$newest  = ($sources | Measure-Object LastWriteTime -Maximum).Maximum
$needsBuild = -not (Test-Path $exe) -or ((Get-Item $exe).LastWriteTime -lt $newest)

if ($needsBuild) {
    Write-Host "Building..."
    dotnet publish "$root\WinOverview.csproj" -c Release -o $out --nologo -v quiet
    if ($LASTEXITCODE -ne 0) { Write-Host "Build failed." -ForegroundColor Red; Read-Host "Press Enter"; exit 1 }
}

Start-Process $exe -WorkingDirectory $out
Write-Host "WinOverview is running - look for its icon in the tray."
