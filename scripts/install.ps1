# Heron installer for Windows. Downloads the release, checks it against the published checksums,
# and installs it for the current user (no administrator rights needed).
#
#   irm https://raw.githubusercontent.com/fernandogarzaaa/HERON-Project/v0.1.0/scripts/install.ps1 | iex
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
    # Close any copy of Heron running from this folder (the app and the command-line tool) and wait
    # until it has really gone: Windows will not replace a program that is still running.
    $prefix = $dest.TrimEnd('\') + '\'
    $findRunning = { Get-Process -Name "Heron", "heron-cli" -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase) } }
    $running = @(& $findRunning)
    if ($running.Count -gt 0) {
        $running | Stop-Process -Force -ErrorAction SilentlyContinue
        $running | Wait-Process -Timeout 20 -ErrorAction SilentlyContinue
        if (@(& $findRunning).Count -gt 0) { throw "Heron is still running and could not be closed. Close it, then run this again. Nothing was changed." }
    }
    # Remove the old version, but keep the uninstaller left by the setup program (unins000.exe and
    # its record), so "Uninstall" in Windows Settings keeps working if Heron was first installed that way.
    if (Test-Path $dest) { Get-ChildItem -LiteralPath $dest -Force | Where-Object { $_.Name -notlike "unins*" } | Remove-Item -Recurse -Force }
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
