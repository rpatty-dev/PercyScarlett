# Builds the PercyScarlett runtime (runtime/victory) and runs your own dump
# into the opening prison scene on the Series S profile.
#
#   .\scripts\play-prologue.ps1 -Game "D:\path\to\your\dump"
#
# -Console series-x   use the Series X profile (slower, more VRAM)
# -NoBuild            skip the cargo build
param(
    [Parameter(Mandatory = $true)][string]$Game,
    [ValidateSet('series-s', 'series-x')][string]$Console = 'series-s',
    [switch]$NoBuild
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$runtime = Join-Path $root 'runtime\victory'
$bin = Join-Path $runtime 'target\release'
$exe = Join-Path $Game 'game_scarlett_master.exe'
if (-not (Test-Path $exe)) { throw "No game_scarlett_master.exe in $Game" }

if (-not $NoBuild) {
    Push-Location $runtime
    cargo build --release -p victory
    if ($LASTEXITCODE -ne 0) { Pop-Location; throw 'build failed' }
    Pop-Location
}

# D3D12 Agility SDK next to the exe (Windows 10 needs it).
$agility = Join-Path $bin 'D3D12'
if (-not (Test-Path (Join-Path $agility 'D3D12Core.dll'))) {
    Write-Host '==> Downloading D3D12 Agility SDK 1.614.1...'
    $tmp = Join-Path $env:TEMP 'percy-agility'
    New-Item -ItemType Directory -Force $tmp | Out-Null
    $pkg = Join-Path $tmp 'agility.zip'
    Invoke-WebRequest 'https://www.nuget.org/api/v2/package/Microsoft.Direct3D.D3D12/1.614.1' -OutFile $pkg
    Expand-Archive -Force $pkg $tmp
    New-Item -ItemType Directory -Force $agility | Out-Null
    Copy-Item (Join-Path $tmp 'build\native\bin\x64\D3D12Core.dll') $agility
    Copy-Item (Join-Path $tmp 'build\native\bin\x64\d3d12SDKLayers.dll') $agility
}

# Levels on demand saves about 3 GB of VRAM.
$env:VICTORY_LEVELS_ON_DEMAND = '1'
if ($Console -eq 'series-x') { $env:VICTORY_DISPLAY_MODE = '1920x1080' }

Push-Location $Game
try {
    & (Join-Path $bin 'victory.exe') run $exe --execute --console $Console
} finally {
    Pop-Location
}
