# Configure a coding font (with Korean + icons) for bbarit's terminal.
# A TUI cannot embed its own font — the terminal renders it — so this installs
# (best effort) and selects one of two fonts on Windows Terminal + the console.
#
#   powershell -ExecutionPolicy Bypass -File scripts/setup-font.ps1 -Font FiraCode
#   powershell -ExecutionPolicy Bypass -File scripts/setup-font.ps1 -Font NanumGothicCoding
param(
    [ValidateSet("FiraCode", "NanumGothicCoding")]
    [string]$Font = "NanumGothicCoding"
)

# Map the choice to (terminal face, direct .ttf urls from the google/fonts repo).
$fonts = @{
    "FiraCode"          = @{
        Face = "Fira Code"
        Ttfs = @("https://github.com/google/fonts/raw/main/ofl/firacode/FiraCode%5Bwght%5D.ttf")
    }
    "NanumGothicCoding" = @{
        Face = "NanumGothicCoding"
        Ttfs = @(
            "https://github.com/google/fonts/raw/main/ofl/nanumgothiccoding/NanumGothicCoding-Regular.ttf",
            "https://github.com/google/fonts/raw/main/ofl/nanumgothiccoding/NanumGothicCoding-Bold.ttf"
        )
    }
}
$pick = $fonts[$Font]
# Fira Code has no Korean glyphs — fall back to NanumGothicCoding for Hangul.
$face = if ($Font -eq "FiraCode") { "Fira Code, NanumGothicCoding" } else { $pick.Face }

Write-Host "bbarit font setup → $Font ('$face')"

function Test-FontInstalled([string]$name) {
    Add-Type -AssemblyName System.Drawing
    return ([System.Drawing.FontFamily]::Families | Where-Object { $_.Name -like "*$name*" }).Count -gt 0
}

function Install-FontTtfs([string[]]$urls) {
    $userFonts = Join-Path $env:LOCALAPPDATA "Microsoft\Windows\Fonts"
    New-Item -ItemType Directory -Force $userFonts | Out-Null
    $reg = "HKCU:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts"
    $installed = 0
    foreach ($url in $urls) {
        try {
            $name = [System.IO.Path]::GetFileName(($url -split '\?')[0]) -replace '%5B', '[' -replace '%5D', ']'
            $dest = Join-Path $userFonts $name
            Write-Host "  downloading $name"
            Invoke-WebRequest -Uri $url -OutFile $dest -UseBasicParsing
            New-ItemProperty -Path $reg -Name "$([System.IO.Path]::GetFileNameWithoutExtension($name)) (TrueType)" -Value $dest -PropertyType String -Force | Out-Null
            $installed++
        } catch {
            Write-Host "  could not fetch $url : $_"
        }
    }
    if ($installed -gt 0) {
        Write-Host "  installed $installed font file(s) for the current user."
    }
    return $installed -gt 0
}

# 1) Install the font if it is missing.
$check = if ($Font -eq "FiraCode") { "Fira Code" } else { "NanumGothicCoding" }
if (Test-FontInstalled $check) {
    Write-Host "  '$check' already installed."
} else {
    if (-not (Install-FontTtfs $pick.Ttfs)) {
        Write-Host "  Install it manually, then re-run. Files: $($pick.Ttfs -join ', ')"
    }
}

# 2) Windows Terminal: set the default profile's font face.
$wt = Join-Path $env:LOCALAPPDATA "Packages\Microsoft.WindowsTerminal_8wekyb3d8bbwe\LocalState\settings.json"
if (Test-Path $wt) {
    try {
        $json = Get-Content $wt -Raw | ConvertFrom-Json
        if (-not $json.profiles.defaults) {
            $json.profiles | Add-Member -NotePropertyName defaults -NotePropertyValue ([pscustomobject]@{}) -Force
        }
        $json.profiles.defaults | Add-Member -NotePropertyName font -NotePropertyValue ([pscustomobject]@{ face = $face }) -Force
        ($json | ConvertTo-Json -Depth 32) | Set-Content $wt -Encoding utf8
        Write-Host "  Windows Terminal font set to '$face' — restart Windows Terminal."
    } catch {
        Write-Host "  Could not edit Windows Terminal settings: $_"
    }
} else {
    Write-Host "  Windows Terminal not found (skipping)."
}

# 3) Legacy console (cmd.exe) default font.
try {
    New-Item -Path "HKCU:\Console" -Force | Out-Null
    Set-ItemProperty -Path "HKCU:\Console" -Name "FaceName" -Value $pick.Face
    Set-ItemProperty -Path "HKCU:\Console" -Name "FontFamily" -Value 54
    Write-Host "  Legacy console font set to '$($pick.Face)' (new cmd windows)."
} catch {
    Write-Host "  Could not set console font: $_"
}

Write-Host ""
Write-Host "Done. Switch font any time:"
Write-Host "  scripts\setup-font.ps1 -Font FiraCode          (ligatures + Korean fallback)"
Write-Host "  scripts\setup-font.ps1 -Font NanumGothicCoding (Korean-first)"
