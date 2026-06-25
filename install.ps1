#!/usr/bin/env pwsh
# npkill-rs Windows installer
# Run: irm https://raw.githubusercontent.com/David-glitc/npkill-rs/main/install.ps1 | iex

$Repo = "David-glitc/npkill-rs"
$BinName = "npkill-rs"
$InstallDir = "$env:USERPROFILE\.local\bin"

function Write-Info  { Write-Host "$args" -ForegroundColor Cyan }
function Write-Ok    { Write-Host "$args" -ForegroundColor Green }
function Write-Bold  { Write-Host "$args" -ForegroundColor White -NoNewline; Write-Host "" }

Write-Bold "npkill-rs installer for Windows"

# ── Detect architecture ───────────────────────────────────────
$Arch = if ([Environment]::Is64BitOperatingSystem) { "x86_64" } else { "i686" }
$Target = "${Arch}-pc-windows-msvc"
Write-Info "Detected target: $Target"

# ── Get latest release tag ────────────────────────────────────
$ApiUrl = "https://api.github.com/repos/$Repo/releases/latest"
try {
    $Release = Invoke-RestMethod -Uri $ApiUrl -ErrorAction Stop
    $Tag = $Release.tag_name
} catch {
    Write-Error "Failed to fetch latest release: $_"
    exit 1
}
Write-Info "Latest release: $Tag"

# ── Download ──────────────────────────────────────────────────
$DownloadUrl = "https://github.com/$Repo/releases/download/$Tag/$BinName-$Target.zip"
$ZipPath = "$env:TEMP\$BinName-$Target.zip"

Write-Bold "Downloading $BinName $Tag..."
try {
    Invoke-WebRequest -Uri $DownloadUrl -OutFile $ZipPath -ErrorAction Stop
} catch {
    Write-Error "Download failed: $_"
    exit 1
}

# ── Extract ───────────────────────────────────────────────────
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
try {
    Expand-Archive -Path $ZipPath -DestinationPath $InstallDir -Force -ErrorAction Stop
} catch {
    Write-Error "Extraction failed: $_"
    exit 1
}
Remove-Item $ZipPath -Force

Write-Ok "Installed $BinName to $InstallDir\$BinName.exe"

# ── Add to PATH ──────────────────────────────────────────────
$UserPath = [Environment]::GetEnvironmentVariable("PATH", "User")
if ($UserPath -notlike "*$InstallDir*") {
    $NewPath = "$InstallDir;$UserPath"
    [Environment]::SetEnvironmentVariable("PATH", $NewPath, "User")
    # Also update for this session
    $env:PATH = "$InstallDir;$env:PATH"
    Write-Ok "Added $InstallDir to your PATH"
} else {
    Write-Info "$InstallDir already in PATH"
}

Write-Host ""
Write-Bold "Usage:"
Write-Host "  $BinName --help"
Write-Host "  $BinName -d C:\path\to\scan"
Write-Host ""
Write-Ok "Done! You may need to restart your terminal for PATH changes to take effect."
