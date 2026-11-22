# Copyright (c) 2026 bunbun-digital and contributors. SPDX-License-Identifier: Apache-2.0
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Iso,
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-fA-F]{64}$')][string]$Sha256,
    [Parameter(Mandatory)][ValidateSet('ja','en')][string]$Language,
    [string]$FixtureDirectory
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$install = 'C:\Program Files\CRISPAWN-SecureBootTest-v2'
if ($FixtureDirectory) {
    $install = (Get-Item -LiteralPath $FixtureDirectory).FullName
    if ($install -ine $PSScriptRoot) { throw 'Run the fixture copy from the fixture directory.' }
}
$request = Join-Path $install 'request.bin'
$id = [Guid]::NewGuid()
$mutex = [Threading.Mutex]::new($false,'Local\CRISPAWN-SubmitSecureBoot-v2')
$locked = $false
try {
    try { $locked = $mutex.WaitOne(0) } catch [Threading.AbandonedMutexException] { $locked = $true }
    if (!$locked) { throw 'Another submission is active.' }
    $inputFile = [IO.File]::Open((Get-Item -LiteralPath $Iso).FullName,'Open','Read','Read')
    try {
        if ($inputFile.Length -lt 1 -or $inputFile.Length -ge 100000000) { throw 'ISO must be smaller than 100 MB.' }
        $digest = [Security.Cryptography.SHA256]::Create()
        try { $bytes = $digest.ComputeHash($inputFile) } finally { $digest.Dispose() }
        if ([BitConverter]::ToString($bytes).Replace('-','') -ine $Sha256) { throw 'Input ISO SHA-256 mismatch.' }
        $inputFile.Position = 0
        $output = [IO.File]::Open($request,'Open','Write','None')
        try {
            $output.SetLength(0)
            $writer = [IO.BinaryWriter]::new($output)
            $writer.Write([Text.Encoding]::ASCII.GetBytes('CRISO002'))
            $writer.Write($id.ToByteArray())
            $writer.Write($bytes)
            $writer.Write([byte]($Language -eq 'en'))
            $writer.Write([uint32]$inputFile.Length)
            $writer.Flush()
            $inputFile.CopyTo($output)
            $output.Flush($true)
        } finally { $output.Dispose() }
    } finally { $inputFile.Dispose() }
    $result = Join-Path $install ('runs\' + $id.ToString('N') + '\submission-result.json')
    if ($FixtureDirectory) {
        # Only the non-elevated fixture runner accepts a test installation directory.
        & (Join-Path $PSScriptRoot 'Receive-SecureBootImage.ps1') -Preflight
    } else {
        & "$env:SystemRoot\System32\schtasks.exe" /Run /TN '\CRISPAWN-SecureBootTest-v2'
        if ($LASTEXITCODE) { throw 'Task Scheduler rejected the start request.' }
    }
    $deadline = [DateTime]::UtcNow.AddMinutes(10)
    while (!(Test-Path -LiteralPath $result) -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Seconds 2 }
    if (!(Test-Path -LiteralPath $result)) { throw "Timed out waiting for $result" }
    $receipt = Get-Content -Raw -LiteralPath $result | ConvertFrom-Json
    if ($receipt.id -cne $id.ToString('N') -or $receipt.iso_sha256 -ine $Sha256 -or
        $receipt.preflight -ne [bool]$FixtureDirectory -or !$receipt.passed) { throw "Submission failed: $result" }
    Write-Output $result
} finally {
    if ($locked) { $mutex.ReleaseMutex() }
    $mutex.Dispose()
}
