# Assembles a self-contained "copy the folder and run" portable distribution:
#
#   <OutputDir>\
#     codex.exe        the portable launcher (config dialog + CDP inject)
#     codex_app\       a copy of an already-installed Codex App
#     data\backup\     created on first run by the launcher
#
# Unlike the NSIS installer (CodexPlusPlus.nsi), this does not register the
# app anywhere or modify Codex App's own files; the whole folder can be moved
# to another machine and run as-is.
#
# Usage:
#   pwsh scripts/installer/windows/package-portable.ps1 -CodexAppDir "C:\Path\To\codex_app" [-OutputDir dist\windows\portable] [-Build]

param(
    [Parameter(Mandatory = $true)]
    [string]$CodexAppDir,

    [string]$OutputDir = "dist/windows/portable",

    [switch]$Build
)

$ErrorActionPreference = "Stop"
$repoRoot = Resolve-Path "$PSScriptRoot/../../.."

if (-not (Test-Path $CodexAppDir)) {
    throw "CodexAppDir not found: $CodexAppDir"
}

if ($Build) {
    Push-Location $repoRoot
    try {
        cargo build --release -p codex-plus-launcher --bin codex
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build failed with exit code $LASTEXITCODE"
        }
    } finally {
        Pop-Location
    }
}

$builtExe = Join-Path $repoRoot "target/release/codex.exe"
if (-not (Test-Path $builtExe)) {
    throw "Built binary not found at $builtExe. Run with -Build, or build it manually first: cargo build --release -p codex-plus-launcher --bin codex"
}

$outputPath = Join-Path $repoRoot $OutputDir
New-Item -ItemType Directory -Force -Path $outputPath | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $outputPath "data/backup") | Out-Null

Copy-Item $builtExe (Join-Path $outputPath "codex.exe") -Force

$appDest = Join-Path $outputPath "codex_app"
if (Test-Path $appDest) {
    Remove-Item $appDest -Recurse -Force
}
Copy-Item $CodexAppDir $appDest -Recurse -Force

Write-Host "Portable build assembled at $outputPath"
Write-Host "First launch of codex.exe will show the config dialog and create config.ini next to the exe."
