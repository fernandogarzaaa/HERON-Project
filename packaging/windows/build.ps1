# Builds dist\Heron-windows-x64.zip and, when Inno Setup is installed, dist\Heron-Setup-windows-x64.exe.
# Run on Windows from the repository root:
#   powershell -ExecutionPolicy Bypass -File packaging\windows\build.ps1
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..\..")
cargo build --release --features desktop --bins
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
$out = "dist\Heron"
if (Test-Path $out) { Remove-Item -Recurse -Force $out }
Remove-Item -Force "dist\Heron-windows-x64.zip", "dist\Heron-Setup-windows-x64.exe" -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $out | Out-Null
Copy-Item "target\release\heron-desktop.exe" "$out\Heron.exe"
Copy-Item "target\release\heron.exe" "$out\heron-cli.exe"
Copy-Item "LICENSE" "$out\LICENSE.txt"
Compress-Archive -Path "$out\*" -DestinationPath "dist\Heron-windows-x64.zip"
Write-Host "Built dist\Heron-windows-x64.zip"

$iscc = @("${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe", "$env:ProgramFiles\Inno Setup 6\ISCC.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1
if ($iscc) {
    $version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches.Groups[1].Value
    & $iscc "/DAppVersion=$version" "packaging\windows\heron.iss"
    if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed" }
    Write-Host "Built dist\Heron-Setup-windows-x64.exe"
} else {
    Write-Host "Inno Setup not found: skipping the installer (the zip is complete on its own)."
}
