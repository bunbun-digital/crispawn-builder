# Copyright (c) 2026 bunbun-digital and contributors. SPDX-License-Identifier: Apache-2.0
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string[]]$BootImage,
    [Parameter(Mandatory)][ValidatePattern('^[a-fA-F0-9]{64}$')][string[]]$Sha256,
    [switch]$Preflight,
    [ValidateSet('ja','en')][string]$Language = 'en'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if ($BootImage.Count -eq 0 -or $BootImage.Count -ne $Sha256.Count) {
    throw 'Supply one SHA-256 for each boot VHDX.'
}
$images = @(for ($index = 0; $index -lt $BootImage.Count; $index++) {
    $file = Get-Item -LiteralPath $BootImage[$index]
    if ($file.PSIsContainer -or $file.Extension -ine '.vhdx' -or
        $file.FullName -notmatch '^[A-Za-z]:\\' -or
        ($file.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw 'Specify a local regular VHDX file.'
    }
    if ((Get-FileHash -LiteralPath $file.FullName).Hash -ine $Sha256[$index]) {
        throw 'Boot VHDX SHA-256 mismatch.'
    }
    $file
})
if ($Preflight) {
    $images | ForEach-Object { Write-Output "Boot VHDX hash checked: $($_.FullName)" }
    return
}

function Invoke-Smoke($image, [string]$digest) {
    $existingVms = @(Get-VM -ErrorAction Stop)
    $nonce = [Guid]::NewGuid().ToString('N')
    $name = 'crispawn-builder-sb-' + $nonce.Substring(0, 12)
    if (@($existingVms | Where-Object Name -CEQ $name).Count -ne 0) { throw 'VM name collision.' }
    $run = New-Item -ItemType Directory -Path (Join-Path $image.DirectoryName $name)
    $report = [ordered]@{
        schema_version = 1
        vm = $name
        media_sha256 = $digest.ToLowerInvariant()
        started_utc = [DateTime]::UtcNow.ToString('o')
        status = 'pending'
        guest = $null
        stopped = $false
        removed = $false
        failure = $null
        cleanup_failure = $null
    }
    $vm = $null
    $pipe = $null
    $reader = $null
    $serial = $null
    try {
        $copy = Join-Path $run.FullName 'boot.vhdx'
        Copy-Item -LiteralPath $image.FullName -Destination $copy
        if ((Get-FileHash -LiteralPath $copy).Hash -ine $digest) {
            throw 'Copied boot VHDX SHA-256 mismatch.'
        }
        $vm = New-VM -Name $name -Path $run.FullName -NoVHD -Generation 2 -MemoryStartupBytes 4GB
        if (@(Get-VMNetworkAdapter -VM $vm | Where-Object SwitchName).Count -ne 0) {
            throw 'Test VM unexpectedly has a network connection.'
        }
        Set-VM -VM $vm -AutomaticCheckpointsEnabled $false -AutomaticStartAction Nothing
        $drive = Add-VMHardDiskDrive -VM $vm -ControllerType SCSI -Path $copy -Passthru
        $source = Join-Path $run.FullName 'blank-source.vhdx'
        New-VHD -Path $source -Dynamic -SizeBytes 1GB | Out-Null
        Add-VMHardDiskDrive -VM $vm -ControllerType SCSI -Path $source | Out-Null
        Set-VMFirmware -VM $vm -FirstBootDevice $drive -EnableSecureBoot On -SecureBootTemplate MicrosoftUEFICertificateAuthority
        $firmware = Get-VMFirmware -VM $vm
        if ([string]$firmware.SecureBoot -ne 'On' -or $firmware.BootOrder[0].Device.Path -ine $copy -or
            $firmware.SecureBootTemplateId -ne [Guid]'272e7447-90a4-4563-a4b9-8e4ab00526ce') {
            throw 'Firmware did not retain Secure Boot and the intended boot device.'
        }
        $pipeName = 'crispawn-builder-' + $nonce
        Set-VMComPort -VM $vm -Number 1 -Path ('\\.\pipe\' + $pipeName)
        $pipe = [IO.Pipes.NamedPipeClientStream]::new('.', $pipeName, [IO.Pipes.PipeDirection]::In, [IO.Pipes.PipeOptions]::Asynchronous)
        $serial = [IO.StreamWriter]::new((Join-Path $run.FullName 'serial.log'), $false, [Text.UTF8Encoding]::new($false))
        $serial.AutoFlush = $true
        Start-VM -VM $vm -ErrorAction Stop | Out-Null
        $pipe.Connect(30000)
        $reader = [IO.StreamReader]::new($pipe)
        $timer = [Diagnostics.Stopwatch]::StartNew()
        $read = $reader.ReadLineAsync()
        while ($timer.Elapsed.TotalSeconds -lt 120 -and $serial.BaseStream.Length -lt 8MB) {
            if (!$read.Wait(500)) { continue }
            $line = $read.GetAwaiter().GetResult()
            if ($null -eq $line) { break }
            $serial.WriteLine($line)
            if ($line.StartsWith('PB_PROBE ', [StringComparison]::Ordinal)) {
                try {
                    $probe = ConvertFrom-Json -InputObject $line.Substring(9) -ErrorAction Stop
                    $secure = $probe.secure_boot
                    if ($probe.schema_version -eq 1 -and $probe.language -ceq $Language -and
                        $probe.gui_ready -is [bool] -and $probe.gui_ready -and
                        $secure.enabled -is [bool] -and $secure.enabled -and
                        $secure.setup_mode -is [bool] -and !$secure.setup_mode -and
                        $secure.shim_validation_disabled -is [bool] -and !$secure.shim_validation_disabled -and
                        $secure.lockdown -match '\[integrity\]|\[confidentiality\]') {
                        $report.guest = $probe
                        break
                    }
                } catch {
                    # A truncated or invalid guest record cannot establish a pass.
                }
            }
            $read = $reader.ReadLineAsync()
        }
        if ($null -eq $report.guest) { throw 'No matching LiveOS Secure Boot evidence was collected.' }
        $report.status = 'passed'
    } catch {
        $report.status = 'failed'
        $report.failure = $_.Exception.Message
    } finally {
        if ($null -ne $pipe) { $pipe.Dispose() }
        if ($null -ne $reader) { $reader.Dispose() }
        if ($null -ne $serial) { $serial.Dispose() }
        if ($null -ne $vm) {
            try {
                $stopTimer = [Diagnostics.Stopwatch]::StartNew()
                $stopRequested = $false
                do {
                    $owned = Get-VM -Id $vm.Id
                    if ($owned.State -eq 'Off') { break }
                    if ($owned.State -eq 'Running' -and !$stopRequested) {
                        Stop-VM -VM $owned -TurnOff -Confirm:$false | Out-Null
                        $stopRequested = $true
                    }
                    Start-Sleep -Milliseconds 350
                } while ($stopTimer.Elapsed.TotalSeconds -lt 30)
                $owned = Get-VM -Id $vm.Id
                $report.stopped = $owned.State -eq 'Off'
                $ownedPath = [IO.Path]::GetFullPath($owned.Path).TrimEnd('\')
                if (!$report.stopped -or $owned.Name -cne $name -or $owned.Generation -ne 2 -or
                    ($ownedPath -ine $run.FullName -and
                        !$ownedPath.StartsWith($run.FullName + '\', [StringComparison]::OrdinalIgnoreCase))) {
                    throw 'VM identity or stopped state could not be verified before removal.'
                }
                Remove-VM -VM $owned -Force | Out-Null
                $report.removed = @((Get-VM -ErrorAction Stop) | Where-Object Id -EQ $vm.Id).Count -eq 0
                if (!$report.removed) { throw 'New test VM still registered after removal.' }
            } catch {
                $report.status = 'failed'
                $report.cleanup_failure = $_.Exception.Message
            }
        }
        $report.finished_utc = [DateTime]::UtcNow.ToString('o')
        $reportPath = Join-Path $run.FullName 'result.json'
        $report | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $reportPath -Encoding utf8
        Write-Host $reportPath
    }
    return ($report.status -eq 'passed')
}

$allPassed = $true
for ($index = 0; $index -lt $images.Count; $index++) {
    if (!(Invoke-Smoke $images[$index] $Sha256[$index])) { $allPassed = $false }
}
if (!$allPassed) { throw 'At least one Secure Boot smoke test failed. See its result.json.' }
