# bbarit-oss installer for Windows.  Usage (PowerShell):
#   irm https://bbarit.com/agent/install.ps1 | iex
# Or with curl.exe (ships with Windows 10+):
#   curl.exe -fsSL https://bbarit.com/agent/install.ps1 -o install.ps1; powershell -ExecutionPolicy Bypass -File install.ps1
#
# Downloads the prebuilt windows-x64 binary into
# %LOCALAPPDATA%\Programs\bbarit-oss (override with BBARIT_INSTALL_DIR) and
# adds that directory to the user PATH.
$ErrorActionPreference = "Stop"

$BaseUrl = if ($env:BBARIT_UPDATE_BASE) { $env:BBARIT_UPDATE_BASE } else { "https://bbarit.com/agent" }
$InstallDir = if ($env:BBARIT_INSTALL_DIR) { $env:BBARIT_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "Programs\bbarit-oss" }

function Say([string]$Message) {
    Write-Host "bbarit-oss" -ForegroundColor Red -NoNewline
    Write-Host " $Message"
}
# `irm | iex` runs in the caller's shell session, so never `exit` — throw instead.
function Fail([string]$Message) {
    Write-Host "bbarit-oss error: $Message" -ForegroundColor Red
    throw "bbarit-oss install failed: $Message"
}

$arch = $env:PROCESSOR_ARCHITECTURE
if ($arch -eq "ARM64") {
    Say "no native arm64 build yet - installing x64 (runs under Windows emulation)"
} elseif ($arch -ne "AMD64") {
    Fail "unsupported architecture: $arch"
}

try { $manifest = Invoke-RestMethod -UseBasicParsing -Uri "$BaseUrl/latest.json" }
catch { Fail "cannot reach $BaseUrl/latest.json" }
$version = $manifest.version
if (-not $version) { Fail "could not read version from manifest" }
$url = $manifest.targets.'windows-x64'
if (-not $url) { $url = "$BaseUrl/dist/$version/bbarit-oss-windows-x64.exe" }

Say "installing v$version (windows-x64) -> $InstallDir\bbarit-oss.exe"
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

# Download next to the final path so the swap is a same-volume rename.
$tmp = Join-Path $InstallDir ".bbarit-oss.download.$PID.exe"
try {
    Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $tmp
    if ((Get-Item $tmp).Length -lt 1024) { Fail "downloaded file looks too small" }
    $exe = Join-Path $InstallDir "bbarit-oss.exe"
    # Can't overwrite a running .exe - move it aside first (same dance as --upgrade).
    if (Test-Path $exe) {
        $old = Join-Path $InstallDir "bbarit-oss-old.exe"
        Remove-Item $old -Force -ErrorAction SilentlyContinue
        Move-Item $exe $old -Force
    }
    Move-Item $tmp $exe -Force
} finally {
    Remove-Item $tmp -Force -ErrorAction SilentlyContinue
}

# Put the install dir on the user PATH so `bbarit-oss` works in new shells.
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
$onPath = ($userPath -split ";" | Where-Object { $_ -eq $InstallDir }).Count -gt 0
if (-not $onPath) {
    $newPath = if ([string]::IsNullOrEmpty($userPath)) { $InstallDir } else { "$userPath;$InstallDir" }
    [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
    Say "added $InstallDir to your user PATH (new terminals pick it up automatically)"
}
$env:Path = "$env:Path;$InstallDir"

Say "installed. Run:  bbarit-oss --help"
