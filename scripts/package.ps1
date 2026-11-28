# Copyright (c) 2026 bunbun-digital and contributors. SPDX-License-Identifier: Apache-2.0
<#
.SYNOPSIS
Builds release binaries and packages the CRISPAWN Builder community edition.

.DESCRIPTION
Builds the UI and the release desktop and CLI executables with the toolchain pinned in
rust-toolchain.toml, runs the core tests, stages the package, signs the executables
through an optional hook, records SHA-256 digests in manifest.json, and runs the USB
writer preflight test from inside the package. The preflight test mocks every storage
cmdlet; this script never accesses or changes a physical disk.

.PARAMETER Destination
Directory that receives crispawn-builder-oss-<UTC>-<commit>\ and LATEST.json.
Defaults to build\package in the repository.

.PARAMETER SignScript
Optional PowerShell script called once per executable as `& SignScript -Path EXE`.
It must sign the file in place (for example with signtool and a timestamp). Without it,
or when it leaves a file unsigned, the script warns and continues. A file whose
signature is present but not valid stops packaging.

.PARAMETER AllowDirty
Package even when the working tree has uncommitted changes. The manifest records it.
#>
[CmdletBinding()]
param(
    [string]$Destination,
    [string]$SignScript,
    [switch]$AllowDirty,
    [switch]$SkipNpmCi
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
if (!(Test-Path -LiteralPath $cargo)) { $cargo = 'cargo' }
if (!$Destination) { $Destination = Join-Path $repo 'build\package' }

function Invoke-Step([string]$name, [scriptblock]$command) {
    Write-Host "==> $name"
    & $command
    if ($LASTEXITCODE -ne 0) { throw "$name failed with exit code $LASTEXITCODE." }
}

Push-Location $repo
try {
    $commit = (git rev-parse HEAD).Trim()
    $short = $commit.Substring(0, 7)
    $dirty = [bool](git status --porcelain)
    if ($dirty -and !$AllowDirty) { throw 'The working tree has uncommitted changes. Commit them or pass -AllowDirty.' }
    if ($SignScript -and !(Test-Path -LiteralPath $SignScript -PathType Leaf)) { throw "Sign script not found: $SignScript" }

    # Cargo reads rust-toolchain.toml; do not pass +stable or another toolchain here.
    if (!$SkipNpmCi) { Invoke-Step 'npm ci' { npm ci } }
    Invoke-Step 'npm run build' { npm run build }
    Invoke-Step 'cargo test' { & $cargo test --locked -p crispawn-builder-core }
    Invoke-Step 'cargo build desktop (release)' { & $cargo build --release --locked -p crispawn-builder-desktop --features custom-protocol }
    Invoke-Step 'cargo build CLI (release)' { & $cargo build --release --locked -p crispawn-builder-core }
    $toolchain = (& $cargo --version).Trim()

    $created = [DateTime]::UtcNow
    $name = 'crispawn-builder-oss-' + $created.ToString("yyyyMMdd'T'HHmmss'Z'") + "-$short"
    $null = New-Item -ItemType Directory -Path $Destination -Force
    $package = Join-Path (Resolve-Path -LiteralPath $Destination).Path $name
    if (Test-Path -LiteralPath $package) { throw "Package directory already exists: $package" }
    $null = New-Item -ItemType Directory -Path $package

    # The desktop executable embeds the UI (custom-protocol), so build\ui is not shipped.
    $executables = @('crispawn-builder-desktop.exe', 'crispawn-builder.exe')
    foreach ($exe in $executables) { Copy-Item -LiteralPath (Join-Path $repo "build\rust\release\$exe") -Destination $package }
    foreach ($doc in @('README.md', 'README.ja.md', 'BUILDING.md', 'CONTRIBUTING.md', 'LICENSE', 'NOTICE')) {
        Copy-Item -LiteralPath (Join-Path $repo $doc) -Destination $package
    }
    Copy-Item -LiteralPath (Join-Path $repo 'assets\crispawn') -Destination (Join-Path $package 'assets') -Recurse
    $tests = Join-Path $package 'tests'
    $null = New-Item -ItemType Directory -Path $tests
    Copy-Item -LiteralPath (Join-Path $repo 'tests\usb-writer-preflight.ps1') -Destination $tests
    Copy-Item -LiteralPath (Join-Path $repo 'crates\builder-core\src\usb_writer.ps1') -Destination $tests

    # Sign before hashing so the manifest describes the files as shipped.
    $signatures = [ordered]@{}
    foreach ($exe in $executables) {
        $path = Join-Path $package $exe
        if ($SignScript) {
            Write-Host "==> sign $exe"
            & $SignScript -Path $path
        }
        $signature = Get-AuthenticodeSignature -LiteralPath $path
        if ($signature.Status -eq 'NotSigned') {
            Write-Warning "$exe is not signed. Windows SmartScreen may warn users. Packaging continues."
        } elseif ($signature.Status -ne 'Valid') {
            throw "$exe has an invalid signature: $($signature.Status) $($signature.StatusMessage)"
        }
        $signatures[$exe] = [ordered]@{
            status = [string]$signature.Status
            signer = if ($signature.SignerCertificate) { $signature.SignerCertificate.Subject } else { $null }
            thumbprint = if ($signature.SignerCertificate) { $signature.SignerCertificate.Thumbprint } else { $null }
            timestamped = [bool]$signature.TimeStamperCertificate
        }
    }

    Write-Host '==> preflight test inside the package'
    $preflight = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $tests 'usb-writer-preflight.ps1')
    if ($LASTEXITCODE -ne 0 -or ($preflight -join "`n") -notmatch 'USB writer preflight mock passed\.') {
        throw "Packaged USB writer preflight test failed: $preflight"
    }

    $files = @(Get-ChildItem -LiteralPath $package -Recurse -File | Sort-Object FullName | ForEach-Object {
        [ordered]@{
            path = $_.FullName.Substring($package.Length + 1).Replace('\', '/')
            bytes = $_.Length
            sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        }
    })
    $utf8 = New-Object Text.UTF8Encoding $false
    $manifest = [ordered]@{
        repository = 'bunbun-digital/crispawn-builder'
        commit = $commit
        dirty = $dirty
        profile = 'release'
        toolchain = $toolchain
        created_utc = $created.ToString('o')
        signatures = $signatures
        files = $files
    }
    [IO.File]::WriteAllText((Join-Path $package 'manifest.json'), ($manifest | ConvertTo-Json -Depth 6), $utf8)

    # Re-read every file against the manifest before pointing LATEST.json at the package.
    foreach ($file in $files) {
        $actual = (Get-FileHash -LiteralPath (Join-Path $package $file.path) -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($actual -ne $file.sha256) { throw "Package file changed after hashing: $($file.path)" }
    }
    $latest = [ordered]@{
        repository = 'bunbun-digital/crispawn-builder'
        commit = $commit
        package = $name
        gui = 'crispawn-builder-desktop.exe'
        cli = 'crispawn-builder.exe'
        signed = @($signatures.Values | Where-Object { $_.status -eq 'Valid' }).Count -eq $executables.Count
        verified_utc = [DateTime]::UtcNow.ToString('o')
        verified_files = $files.Count
    }
    [IO.File]::WriteAllText((Join-Path (Split-Path $package) 'LATEST.json'), ($latest | ConvertTo-Json), $utf8)
    Write-Host "Package created: $package ($($files.Count) files)"
} finally {
    Pop-Location
}
