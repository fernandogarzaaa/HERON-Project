# Heron installer for Windows. Downloads the release, checks it against the published checksums,
# and installs it for the current user (no administrator rights needed).
#
#   irm https://raw.githubusercontent.com/fernandogarzaaa/HERON-Project/main/scripts/install.ps1 | iex
#
# Set $env:HERON_VERSION to install a different release (default below).
$ErrorActionPreference = "Stop"
$version = if ($env:HERON_VERSION) { $env:HERON_VERSION } else { "v0.1.0" }
$base = if ($env:HERON_DOWNLOAD_BASE) { $env:HERON_DOWNLOAD_BASE } else { "https://github.com/fernandogarzaaa/HERON-Project/releases/download/$version" }
$asset = "Heron-windows-x64.zip"
$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("heron-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force $tmp | Out-Null
try {
    Write-Host "Downloading Heron $version..."
    Invoke-WebRequest -UseBasicParsing -Uri "$base/$asset" -OutFile (Join-Path $tmp $asset)
    Invoke-WebRequest -UseBasicParsing -Uri "$base/SHA256SUMS.txt" -OutFile (Join-Path $tmp "SHA256SUMS.txt")

    Write-Host "Checking the download..."
    $line = Get-Content (Join-Path $tmp "SHA256SUMS.txt") | Where-Object { $_ -match ([regex]::Escape($asset) + '$') } | Select-Object -First 1
    $expected = if ($line) { ($line -split '\s+')[0].ToLower() } else { "" }
    $actual = (Get-FileHash -Algorithm SHA256 (Join-Path $tmp $asset)).Hash.ToLower()
    if (-not $expected -or $expected -ne $actual) { throw "The download does not match the published checksum, so nothing was installed." }

    $dest = Join-Path $env:LOCALAPPDATA "Programs\Heron"
    Get-Process -Name "Heron" -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    if (Test-Path $dest) { Remove-Item -Recurse -Force $dest }
    New-Item -ItemType Directory -Force $dest | Out-Null
    Expand-Archive -Path (Join-Path $tmp $asset) -DestinationPath $dest -Force

    $shell = New-Object -ComObject WScript.Shell
    $shortcut = $shell.CreateShortcut((Join-Path ([Environment]::GetFolderPath("Programs")) "Heron.lnk"))
    $shortcut.TargetPath = Join-Path $dest "Heron.exe"
    $shortcut.WorkingDirectory = $dest
    $shortcut.Description = "Heron - personal AI agent"
    $shortcut.Save()

    Write-Host "Heron is installed. Open it from the Start menu."
    Write-Host "The command-line tool is at $dest\heron-cli.exe"
    Write-Host "Windows may show a SmartScreen notice the first time, because this beta is not code-signed: choose More info, then Run anyway."
} finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
