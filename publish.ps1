# Builds the release: ONE small file, dist\WinOverview.exe (VirtualDesktopAccessor.dll is packed inside).
# It needs the .NET 10 Desktop Runtime on the PC; if that's missing, Windows offers the download.
# Give that file (and uninstall.bat) to anyone - they just double-click it.
#
#   .\publish.ps1

$ErrorActionPreference = "Stop"
$root = $PSScriptRoot
$dist = Join-Path $root "dist"
$build = Join-Path $dist "build"

if (-not (Test-Path "$root\VirtualDesktopAccessor.dll")) {
    Write-Host "VirtualDesktopAccessor.dll is missing from the project folder." -ForegroundColor Red
    exit 1
}

if (Test-Path $build) { Remove-Item $build -Recurse -Force }
New-Item -ItemType Directory -Force -Path $dist | Out-Null

Write-Host "Building single-file WinOverview.exe (takes a minute)..."
dotnet publish "$root\WinOverview.csproj" -c Release -r win-x64 -p:PublishSingleFile=true -o $build --nologo -v quiet
if ($LASTEXITCODE -ne 0) { Write-Host "Publish failed." -ForegroundColor Red; exit 1 }

Copy-Item "$build\WinOverview.exe" "$dist\WinOverview.exe" -Force
Copy-Item "$root\uninstall.bat" "$dist\uninstall.bat" -Force
Remove-Item $build -Recurse -Force

$mb = [math]::Round((Get-Item "$dist\WinOverview.exe").Length / 1MB, 1)
Write-Host "Done: $dist\WinOverview.exe ($mb MB)" -ForegroundColor Green
