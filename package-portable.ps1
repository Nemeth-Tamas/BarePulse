$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repositoryRoot = $PSScriptRoot

$releaseExe = Join-Path `
    $repositoryRoot `
    "target\release\barepulse.exe"

$sourceManifest = Join-Path `
    $repositoryRoot `
    "devices\manifest.toml"

$distRoot = Join-Path `
    $repositoryRoot `
    "dist\BarePulse"

$distDevices = Join-Path `
    $distRoot `
    "devices"

$distExe = Join-Path `
    $distRoot `
    "BarePulse.exe"

$distManifest = Join-Path `
    $distDevices `
    "manifest.toml"

Push-Location $repositoryRoot

try {
    Write-Host "Building BarePulse release..."

    cargo rustc --release -- -D warnings

    if ($LASTEXITCODE -ne 0) {
        throw "release build failed with exit code $LASTEXITCODE"
    }

    if (-not (Test-Path $releaseExe -PathType Leaf)) {
        throw "Release executable was not produced: $releaseExe"
    }

    if (-not (Test-Path $sourceManifest -PathType Leaf)) {
        throw "Device manifest does not exist: $sourceManifest"
    }

    if (Test-Path $distRoot) {
        Remove-Item `
            $distRoot `
            -Recurse `
            -Force
    }

    New-Item `
        -ItemType Directory `
        -Path $distDevices `
        -Force `
        | Out-Null

    Copy-Item `
        $releaseExe `
        $distExe

    Copy-Item `
        $sourceManifest `
        $distManifest

    $unexpectedProfiles = Get-ChildItem `
        $distDevices `
        -Filter "*.toml" `
        -File `
        | Where-Object {
            $_.Name -ne "manifest.toml"
        }

    if ($unexpectedProfiles) {
        throw "Portable package unexpectedly contains device profiles"
    }

    if (Test-Path (Join-Path $distRoot "barepulse.toml")) {
        throw "Portable package unexpectedly contains a generated config"
    }

    Write-Host ""
    Write-Host "Portable BarePulse package ready:"
    Write-Host "  $distRoot"
    Write-Host ""
    Write-Host "Contents:"

    Get-ChildItem `
        $distRoot `
        -Recurse `
        -Force `
        | ForEach-Object {
            Write-Host "  $($_.FullName.Substring($distRoot.Length + 1))"
        }
}
finally {
    Pop-Location
}