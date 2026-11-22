# Copyright (c) 2026 bunbun-digital and contributors. SPDX-License-Identifier: Apache-2.0
# Install only onto an existing CRISPAWN GPT/FAT32/NTFS removable USB.
[CmdletBinding()]
param([Parameter(Mandatory)][string]$RequestPath, [switch]$Elevated, [switch]$Preflight)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$request = Get-Content -LiteralPath $RequestPath -Raw | ConvertFrom-Json
$expectedKeys = @('iso','efi_dir','iso_sha256','efi_sha256','disk_number','disk_unique_id','disk_bytes','report_dir')
$actualKeys = @($request.PSObject.Properties.Name | Sort-Object) -join ','
$baseKeys = @($expectedKeys | Sort-Object) -join ','
$configuredKeys = @($expectedKeys + @('config_path','config_sha256') | Sort-Object) -join ','
if ($actualKeys -cne $baseKeys -and $actualKeys -cne $configuredKeys) {
    throw 'Invalid Builder USB request.'
}
if ($request.iso_sha256 -cnotmatch '^[0-9a-f]{64}$' -or
    $request.disk_number -lt 1 -or $request.disk_bytes -lt 1000000000) {
    throw 'Invalid Builder USB identity or digest.'
}
function Assert-Target {
    $disk = Get-Disk -Number ([int]$request.disk_number) -ErrorAction Stop
    if ($disk.UniqueId -cne $request.disk_unique_id -or
        $disk.Size -ne [UInt64]$request.disk_bytes -or
        $disk.BusType -ne 'USB' -or $disk.PartitionStyle -ne 'GPT' -or
        $disk.IsBoot -or $disk.IsSystem -or $disk.IsOffline -or $disk.IsReadOnly) {
        throw 'Selected USB identity or safety state changed.'
    }
    $parts = @(Get-Partition -DiskNumber $disk.Number -ErrorAction Stop)
    if ($parts.Count -ne 2 -or $parts[0].PartitionNumber -ne 1 -or
        $parts[1].PartitionNumber -ne 2 -or
        $parts[0].GptType -ine '{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}' -or
        $parts[1].GptType -ine '{ebd0a0a2-b9e5-4433-87c0-68b6b72699c7}' -or
        $parts[0].Size -ne 256MB) {
        throw 'USB is not the prepared CRISPAWN layout.'
    }
    $dataVolume = Get-Volume -Partition $parts[1]
    if ($dataVolume.FileSystem -ne 'NTFS' -or $dataVolume.FileSystemLabel -notin @('PERFECTBACKUP','CRISPAWN')) {
        throw 'USB filesystem or label changed.'
    }
    return $disk
}
function FileHash([string]$path) {
    $stream = [IO.File]::Open($path, 'Open', 'Read', 'Read')
    $sha = [Security.Cryptography.SHA256]::Create()
    try {
        return [BitConverter]::ToString($sha.ComputeHash($stream)).Replace('-','').ToLowerInvariant()
    } finally {
        $sha.Dispose()
        $stream.Dispose()
    }
}
function Report-Progress([string]$phase, [int]$percent) {
    $record = [ordered]@{ phase = $phase; percent = $percent } | ConvertTo-Json -Compress
    [IO.File]::WriteAllText((Join-Path $request.report_dir 'progress.json'),
        $record, [Text.UTF8Encoding]::new($false))
}
function Copy-IsoWithProgress([string]$sourcePath, [string]$targetPath) {
    $source = [IO.File]::Open($sourcePath, 'Open', 'Read', 'Read')
    try {
        $target = [IO.File]::Open($targetPath, 'CreateNew', 'Write', 'None')
        try {
            $buffer = New-Object byte[] (1024 * 1024)
            [long]$written = 0
            do {
                $count = $source.Read($buffer, 0, $buffer.Length)
                if ($count -gt 0) {
                    $target.Write($buffer, 0, $count)
                    $written += $count
                    Report-Progress 'writingIso' (25 + [int][Math]::Floor($written * 45 / $source.Length))
                }
            } while ($count -gt 0)
            $target.Flush($true)
        } finally { $target.Dispose() }
    } finally { $source.Dispose() }
}
function Assert-Inputs {
    $iso = Get-Item -LiteralPath $request.iso -ErrorAction Stop
    if (!$iso -or $iso.Length -le 0 -or $iso.Length -ge 100000000 -or
        (FileHash $request.iso) -cne $request.iso_sha256) { throw 'ISO hash or size changed.' }
    foreach ($name in @('shimx64.efi','grubx64.efi','mmx64.efi')) {
        $expected = $request.efi_sha256.PSObject.Properties[$name].Value
        if ($expected -cnotmatch '^[0-9a-f]{64}$' -or
            (FileHash (Join-Path $request.efi_dir $name)) -cne $expected) {
            throw 'Signed EFI file changed.'
        }
    }
    if ($actualKeys -ceq $configuredKeys) {
        $expectedConfig = Join-Path ([IO.Path]::GetFullPath([string]$request.report_dir)) 'live-config.json'
        if ([IO.Path]::GetFullPath([string]$request.config_path) -cne $expectedConfig -or
            $request.config_sha256 -cnotmatch '^[0-9a-f]{64}$') {
            throw 'Invalid LiveOS settings path or digest.'
        }
        $configFile = Get-Item -LiteralPath $expectedConfig -Force -ErrorAction Stop
        if ($configFile.Attributes -band [IO.FileAttributes]::ReparsePoint -or
            $configFile.Length -lt 1 -or $configFile.Length -gt 65536 -or
            (FileHash $expectedConfig) -cne $request.config_sha256) {
            throw 'LiveOS settings changed.'
        }
    }
}
function Read-Sector([UInt64]$offset) {
    $disk = Assert-Target
    $stream = [IO.File]::Open(('\\.\PhysicalDrive' + $disk.Number), 'Open', 'Read', 'ReadWrite')
    try {
        $null = $stream.Seek([Int64]$offset, 'Begin')
        $bytes = New-Object byte[] 512
        if ($stream.Read($bytes,0,512) -ne 512) { throw 'Short boot sector read.' }
        return ,$bytes
    } finally { $stream.Dispose() }
}
Assert-Inputs
$disk = Assert-Target
$principal = [Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())
if ($Preflight) { Write-Output 'USB_PREFLIGHT_OK'; return }
if (!$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    if ($Elevated) { throw 'USB writer did not receive administrator privileges.' }
    Report-Progress 'awaitingApproval' 18
    $arguments = '-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "' + $PSCommandPath + '" -RequestPath "' + $RequestPath + '" -Elevated'
    $child = Start-Process -FilePath 'powershell.exe' -ArgumentList $arguments -Verb RunAs -WindowStyle Hidden -Wait -PassThru
    exit $child.ExitCode
}
$reportDir = [IO.Path]::GetFullPath([string]$request.report_dir)
if (!(Test-Path -LiteralPath $reportDir -PathType Container)) { throw 'Report directory changed.' }
Report-Progress 'preparing' 20
$resultPath = Join-Path $reportDir 'result.json'
if (Test-Path -LiteralPath $resultPath) { throw 'USB result already exists.' }
$mount = Join-Path $reportDir 'esp'
if (Test-Path -LiteralPath $mount) { throw 'ESP mount directory already exists.' }
$null = New-Item -ItemType Directory -Path $mount -ErrorAction Stop
$mounted = $false
$dataMount = Join-Path $reportDir 'data'
if (Test-Path -LiteralPath $dataMount) { throw 'Data mount directory already exists.' }
$null = New-Item -ItemType Directory -Path $dataMount -ErrorAction Stop
$dataMounted = $false
try {
    Assert-Inputs
    $disk = Assert-Target
    $esp = Get-Partition -DiskNumber $disk.Number -PartitionNumber 1
    $data = Get-Partition -DiskNumber $disk.Number -PartitionNumber 2
    $fat = Read-Sector $esp.Offset
    $ntfs = Read-Sector $data.Offset
    if ([Text.Encoding]::ASCII.GetString($fat,82,8) -cne 'FAT32   ' -or
        [Text.Encoding]::ASCII.GetString($ntfs,3,8) -cne 'NTFS    ') {
        throw 'USB boot sector signatures changed.'
    }
    $bootUuid = [BitConverter]::ToUInt32($fat,67).ToString('X8').Insert(4,'-')
    $dataUuid = [BitConverter]::ToUInt64($ntfs,72).ToString('X16')
    Add-PartitionAccessPath -DiskNumber (Assert-Target).Number -PartitionNumber 2 -AccessPath $dataMount
    $dataMounted = $true
    $legacyDirectory = Join-Path $dataMount 'perfect-backup'
    $crispawnDirectory = Join-Path $dataMount 'crispawn'
    foreach ($directory in @($legacyDirectory, $crispawnDirectory)) {
        if (Test-Path -LiteralPath $directory) {
            $entry = Get-Item -LiteralPath $directory -Force
            if (!$entry.PSIsContainer -or ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
                throw 'USB data directory is not a regular directory.'
            }
        }
    }
    if ((Test-Path -LiteralPath $legacyDirectory) -and (Test-Path -LiteralPath $crispawnDirectory)) {
        throw 'Both legacy and CRISPAWN data directories exist; refusing to merge them.'
    }
    Add-PartitionAccessPath -DiskNumber (Assert-Target).Number -PartitionNumber 1 -AccessPath $mount
    $mounted = $true
    Report-Progress 'writingIso' 25
    $isoName = 'crispawn-' + $request.iso_sha256.Substring(0,12) + '.iso'
    $isoTarget = Join-Path $mount $isoName
    if (Test-Path -LiteralPath $isoTarget) {
        if ((FileHash $isoTarget) -cne $request.iso_sha256) { throw 'Existing ISO name has different bytes.' }
    } else {
        $null = Assert-Target
        Copy-IsoWithProgress $request.iso $isoTarget
    }
    Report-Progress 'verifyingIso' 73
    if ((FileHash $isoTarget) -cne $request.iso_sha256) { throw 'ISO media readback mismatch.' }
    Report-Progress 'verifyingIso' 82
    foreach ($folder in @('EFI\BOOT','EFI\debian','boot\grub')) {
        $null = [IO.Directory]::CreateDirectory((Join-Path $mount $folder))
    }
    $files = @{
        'EFI\BOOT\BOOTX64.EFI' = 'shimx64.efi'
        'EFI\BOOT\grubx64.efi' = 'grubx64.efi'
        'EFI\BOOT\mmx64.efi' = 'mmx64.efi'
    }
    $bootCount = 0
    foreach ($entry in $files.GetEnumerator()) {
        Report-Progress 'writingBoot' (84 + $bootCount * 3)
        $null = Assert-Target
        $destination = Join-Path $mount $entry.Key
        Copy-Item -LiteralPath (Join-Path $request.efi_dir $entry.Value) -Destination $destination -Force -ErrorAction Stop
        if ((FileHash $destination) -cne $request.efi_sha256.PSObject.Properties[$entry.Value].Value) {
            throw 'EFI media readback mismatch.'
        }
        $bootCount++
    }
    Report-Progress 'writingBoot' 93
    $cfg = @"
serial --unit=0 --speed=115200
terminal_output serial
echo PB_USB_GRUB_START uuid=$bootUuid iso=$isoName
search --no-floppy --fs-uuid --set=pbroot $bootUuid
loopback loop (`$pbroot)/$isoName
linux (loop)/boot/bzImage quiet ignore_loglevel console=ttyS0,115200n8 logo.nologo vt.global_cursor_default=0 net.ifnames=0 rdinit=/init pb.live=1 pb.boot_uuid=$bootUuid pb.data_uuid=$dataUuid pb.require_secure_boot=1 module.sig_enforce=1
initrd (loop)/boot/initrd
boot
echo PB_USB_BOOT_FAILED
sleep 30
"@
    foreach ($path in @('EFI\BOOT\grub.cfg','EFI\debian\grub.cfg','boot\grub\grub.cfg')) {
        $null = Assert-Target
        $destination = Join-Path $mount $path
        [IO.File]::WriteAllText($destination,$cfg,[Text.Encoding]::ASCII)
        if ([IO.File]::ReadAllText($destination) -cne $cfg) { throw 'GRUB media readback mismatch.' }
    }
    if ($actualKeys -ceq $configuredKeys) {
        $null = Assert-Target
        $configTarget = Join-Path $mount 'perfect-backup.json'
        Copy-Item -LiteralPath $request.config_path -Destination $configTarget -Force -ErrorAction Stop
        if ((FileHash $configTarget) -cne $request.config_sha256) {
            throw 'LiveOS settings readback mismatch.'
        }
    }
    Report-Progress 'verifyingFinal' 95
    if ((FileHash $isoTarget) -cne $request.iso_sha256) { throw 'Final ISO readback mismatch.' }
    if (Test-Path -LiteralPath $legacyDirectory) {
        $null = Assert-Target
        Move-Item -LiteralPath $legacyDirectory -Destination $crispawnDirectory -ErrorAction Stop
    } elseif (!(Test-Path -LiteralPath $crispawnDirectory)) {
        $null = Assert-Target
        $null = New-Item -ItemType Directory -Path $crispawnDirectory -ErrorAction Stop
    }
    $dataVolume = Get-Volume -Partition (Get-Partition -DiskNumber (Assert-Target).Number -PartitionNumber 2)
    if ($dataVolume.FileSystemLabel -cne 'CRISPAWN') {
        Set-Volume -InputObject $dataVolume -NewFileSystemLabel CRISPAWN
    }
    if ((Get-Volume -Partition (Get-Partition -DiskNumber (Assert-Target).Number -PartitionNumber 2)).FileSystemLabel -cne 'CRISPAWN' -or
        !(Test-Path -LiteralPath $crispawnDirectory -PathType Container)) {
        throw 'CRISPAWN data volume rename could not be verified.'
    }
    [ordered]@{
        schema_version = 1
        disk_number = [int]$request.disk_number
        disk_unique_id = [string]$request.disk_unique_id
        disk_bytes = [UInt64]$request.disk_bytes
        boot_uuid = $bootUuid
        data_uuid = $dataUuid
        iso_name = $isoName
        iso_sha256 = [string]$request.iso_sha256
        config_sha256 = if ($actualKeys -ceq $configuredKeys) { [string]$request.config_sha256 } else { $null }
        network_profile = 'none'
        ntfs_preserved = $true
        data_volume_label = 'CRISPAWN'
        data_directory = 'crispawn'
        readback_verified = $true
    } | ConvertTo-Json -Depth 4 | ForEach-Object {
        [IO.File]::WriteAllText($resultPath, $_, [Text.UTF8Encoding]::new($false))
    }
    Report-Progress 'verifyingFinal' 99
} catch {
    $_.Exception.Message | Set-Content -LiteralPath (Join-Path $reportDir 'failure.txt') -Encoding UTF8
    throw
} finally {
    try {
        if ($dataMounted) {
            Remove-PartitionAccessPath -DiskNumber (Assert-Target).Number -PartitionNumber 2 -AccessPath $dataMount
        }
    } finally {
        if ($mounted) {
            Remove-PartitionAccessPath -DiskNumber (Assert-Target).Number -PartitionNumber 1 -AccessPath $mount
        }
    }
}
