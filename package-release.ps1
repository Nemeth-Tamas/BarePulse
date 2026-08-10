$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repositoryRoot = $PSScriptRoot

$cargoManifest = Join-Path `
    $repositoryRoot `
    "Cargo.toml"

$portableScript = Join-Path `
    $repositoryRoot `
    "package-portable.ps1"

$portableRoot = Join-Path `
    $repositoryRoot `
    "dist\BarePulse"

$installerScript = Join-Path `
    $repositoryRoot `
    "installer\BarePulse.iss"

$distRoot = Join-Path `
    $repositoryRoot `
    "dist"

function Get-BarePulseVersion {
    $contents = Get-Content `
        $cargoManifest `
        -Raw

    $match = [regex]::Match(
        $contents,
        '(?m)^\s*version\s*=\s*"([^"]+)"'
    )

    if (-not $match.Success) {
        throw "Could not determine BarePulse version from Cargo.toml"
    }

    return $match.Groups[1].Value
}

function Find-InnoSetupCompiler {
    $command = Get-Command `
        "ISCC.exe" `
        -ErrorAction SilentlyContinue

    if ($null -ne $command) {
        return $command.Source
    }

    $candidates = @()

    if (${env:ProgramFiles(x86)}) {
        $candidates += Join-Path `
            ${env:ProgramFiles(x86)} `
            "Inno Setup 7\ISCC.exe"

        $candidates += Join-Path `
            ${env:ProgramFiles(x86)} `
            "Inno Setup 6\ISCC.exe"
    }

    if ($env:ProgramFiles) {
        $candidates += Join-Path `
            $env:ProgramFiles `
            "Inno Setup 7\ISCC.exe"

        $candidates += Join-Path `
            $env:ProgramFiles `
            "Inno Setup 6\ISCC.exe"
    }

    if ($env:LOCALAPPDATA) {
        $candidates += Join-Path `
            $env:LOCALAPPDATA `
            "Programs\Inno Setup 7\ISCC.exe"

        $candidates += Join-Path `
            $env:LOCALAPPDATA `
            "Programs\Inno Setup 6\ISCC.exe"
    }

    foreach ($candidate in $candidates) {
        if (Test-Path $candidate -PathType Leaf) {
            return $candidate
        }
    }

    throw @"
Inno Setup compiler ISCC.exe was not found.

Install Inno Setup, then run package-release.ps1 again.
"@
}

Push-Location $repositoryRoot

try {
    $version = Get-BarePulseVersion

    Write-Host "Packaging BarePulse v$version..."
    Write-Host ""

    & $portableScript

    if ($LASTEXITCODE -ne 0) {
        throw "Portable packaging failed with exit code $LASTEXITCODE"
    }

    $portableArchive = Join-Path `
        $distRoot `
        "BarePulse-v$version-Portable.zip"

    if (Test-Path $portableArchive) {
        Remove-Item `
            $portableArchive `
            -Force
    }

    Write-Host ""
    Write-Host "Creating portable archive..."

    Compress-Archive `
        -Path (Join-Path $portableRoot "*") `
        -DestinationPath $portableArchive `
        -CompressionLevel Optimal

    if (-not (Test-Path $portableArchive -PathType Leaf)) {
        throw "Portable archive was not created"
    }

    $iscc = Find-InnoSetupCompiler

    Write-Host ""
    Write-Host "Building installer with:"
    Write-Host "  $iscc"

    $previousPackageVersion = `
        $env:BAREPULSE_PACKAGE_VERSION

    try {
        $env:BAREPULSE_PACKAGE_VERSION = $version

        & $iscc $installerScript

        if ($LASTEXITCODE -ne 0) {
            throw "Inno Setup failed with exit code $LASTEXITCODE"
        }
    }
    finally {
        if ($null -eq $previousPackageVersion) {
            Remove-Item `
                Env:BAREPULSE_PACKAGE_VERSION `
                -ErrorAction SilentlyContinue
        }
        else {
            $env:BAREPULSE_PACKAGE_VERSION = `
                $previousPackageVersion
        }
    }

    $installer = Join-Path `
        $distRoot `
        "BarePulse-v$version-Setup.exe"

    if (-not (Test-Path $installer -PathType Leaf)) {
        throw "Installer was not created: $installer"
    }

    Write-Host ""
    Write-Host "BarePulse release artifacts ready:"
    Write-Host "  $portableArchive"
    Write-Host "  $installer"
}
finally {
    Pop-Location
}