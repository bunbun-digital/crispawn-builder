# Copyright (c) 2026 bunbun-digital and contributors. SPDX-License-Identifier: Apache-2.0
# Install onto a selected USB bus disk; prepare an unformatted disk only with explicit request.
[CmdletBinding()]
param([Parameter(Mandatory)][string]$RequestPath, [switch]$Elevated, [switch]$Preflight)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
try {
$request = Get-Content -LiteralPath $RequestPath -Raw | ConvertFrom-Json
$expectedKeys = @('iso','efi_dir','iso_sha256','efi_sha256','disk_number','disk_unique_id','disk_bytes','report_dir')
$actualKeys = @($request.PSObject.Properties.Name | Sort-Object) -join ','
$baseKeys = @($expectedKeys | Sort-Object) -join ','
$selectedKeys = @('disk_path','serial_number','layout_stamp','initialize')
$selectedBaseKeys = @($expectedKeys + $selectedKeys | Sort-Object) -join ','
if ($actualKeys -cnotin @($baseKeys,$selectedBaseKeys)) {
    throw 'Invalid Builder USB request.'
}
$hasSelection = $actualKeys -ceq $selectedBaseKeys
# LiveOS reads settings from the boot volume before the ISO's own; the Builder embeds them in the ISO.
$EspSettingsNames = @('crispawn-config.json','perfect-backup.json')
$initialize = $hasSelection -and [bool]$request.initialize
if ($request.iso_sha256 -cnotmatch '^[0-9a-f]{64}$' -or
    $request.disk_number -lt 0 -or $request.disk_bytes -lt 536870912) {
    throw 'Invalid Builder USB identity or digest.'
}
function Assert-Identity([bool]$checkLayout) {
    $disk = Get-Disk -Number ([int]$request.disk_number) -ErrorAction Stop
    if ([string]$disk.UniqueId -cne [string]$request.disk_unique_id -or
        $disk.Size -ne [UInt64]$request.disk_bytes -or
        $disk.BusType -ne 'USB' -or
        $disk.IsBoot -or $disk.IsSystem -or $disk.IsOffline -or $disk.IsReadOnly) {
        throw 'Selected USB identity or safety state changed.'
    }
    if ($hasSelection) {
        if ([string]$disk.Path -cne [string]$request.disk_path -or [string]$disk.SerialNumber -cne [string]$request.serial_number) {
            throw 'Selected USB hardware identity changed.'
        }
        if ($checkLayout) {
            $layoutParts = @(Get-Partition -DiskNumber $disk.Number -ErrorAction SilentlyContinue | Sort-Object PartitionNumber)
            $layout = [string]$disk.PartitionStyle + '|' + (($layoutParts | ForEach-Object {
                [string]$_.PartitionNumber + ':' + [string]$_.Offset + ':' + [string]$_.Size + ':' + [string]$_.GptType + ':' + [string]$_.MbrType
            }) -join '|')
            if ($layout -cne $request.layout_stamp) {
                throw 'Selected USB partition layout changed. Refresh the USB list.'
            }
        }
    }
    return $disk
}
# Windows adds a Microsoft Reserved partition when it initializes a fixed USB disk
# (HDD/SSD) as GPT; removable flash media get none. Accept it only before the ESP.
$MsrType = '{e3c9e316-0b5c-4db8-817d-f92df00215ae}'
$EspType = '{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}'
$DataType = '{ebd0a0a2-b9e5-4433-87c0-68b6b72699c7}'
$script:targetEsp = $null
$script:targetData = $null
function Assert-Target {
    $disk = Assert-Identity (!$initialize)
    if ($disk.PartitionStyle -ne 'GPT') { throw 'USB is not GPT.' }
    $parts = @(Get-Partition -DiskNumber $disk.Number -ErrorAction Stop | Sort-Object PartitionNumber)
    if ($parts.Count -eq 3 -and $parts[0].GptType -ieq $MsrType -and $parts[0].Size -le 128MB) {
        $parts = @($parts[1], $parts[2])
    }
    if ($parts.Count -ne 2 -or
        $parts[0].GptType -ine $EspType -or
        $parts[1].GptType -ine $DataType -or
        $parts[0].Size -ne 256MB) {
        throw 'USB is not the prepared CRISPAWN layout.'
    }
    $script:targetEsp = $parts[0]
    $script:targetData = $parts[1]
    $dataVolume = Get-Volume -Partition $parts[1]
    if ($dataVolume.FileSystem -ne 'NTFS' -or $dataVolume.FileSystemLabel -notin @('PERFECTBACKUP','CRISPAWN')) {
        throw 'USB filesystem or label changed.'
    }
    return $disk
}
function Get-DataVolumeDirty {
    $disk = Assert-Target
    $partition = Get-Partition -DiskNumber $disk.Number -PartitionNumber $script:targetData.PartitionNumber -ErrorAction Stop
    $volume = Get-Volume -Partition $partition -ErrorAction Stop
    if ($volume.FileSystem -ne 'NTFS' -or !$volume.Path) {
        throw 'USB_NTFS_STATE_UNKNOWN: The data volume could not be identified.'
    }
    $volumePath = ([string]$volume.Path).TrimEnd('\')
    $matches = @(Get-CimInstance -ClassName Win32_Volume -ErrorAction Stop | Where-Object {
        ([string]$_.DeviceID).TrimEnd('\') -ieq $volumePath
    })
    if ($matches.Count -ne 1 -or $null -eq $matches[0].DirtyBitSet) {
        throw 'USB_NTFS_STATE_UNKNOWN: Windows did not report the data volume dirty state.'
    }
    return [bool]$matches[0].DirtyBitSet
}
function Wait-DataVolumeClean {
    for ($attempt = 0; $attempt -lt 10; $attempt++) {
        if (!(Get-DataVolumeDirty)) { return $true }
        Start-Sleep -Milliseconds 500
    }
    return $false
}
function Initialize-SelectedUsb {
    $disk = Assert-Identity $true
    Report-Progress 'initializing' 21
    $parts = @(Get-Partition -DiskNumber $disk.Number -ErrorAction SilentlyContinue)
    if ($parts.Count -gt 0) {
        Clear-Disk -Number $disk.Number -RemoveData -RemoveOEM -Confirm:$false -ErrorAction Stop
    }
    $disk = Assert-Identity $false
    $remaining = @(Get-Partition -DiskNumber $disk.Number -ErrorAction SilentlyContinue)
    if ($remaining.Count -ne 0) { throw 'USB still has partitions after clearing.' }
    switch ([string]$disk.PartitionStyle) {
        'RAW' { Initialize-Disk -Number $disk.Number -PartitionStyle GPT -ErrorAction Stop }
        'MBR' { Set-Disk -Number $disk.Number -PartitionStyle GPT -ErrorAction Stop }
        'GPT' { }
        default { throw 'Unsupported USB partition style.' }
    }
    $disk = Assert-Identity $false
    if ($disk.PartitionStyle -ne 'GPT') { throw 'USB could not be converted to GPT.' }
    $newEsp = New-Partition -DiskNumber $disk.Number -Size 256MB -GptType $EspType -ErrorAction Stop
    $null = Format-Volume -Partition $newEsp -FileSystem FAT32 -NewFileSystemLabel CRISPAWN -Confirm:$false -ErrorAction Stop
    $newData = New-Partition -DiskNumber $disk.Number -UseMaximumSize -GptType $DataType -ErrorAction Stop
    $null = Format-Volume -Partition $newData -FileSystem NTFS -NewFileSystemLabel CRISPAWN -Confirm:$false -ErrorAction Stop
}
function Remove-EspSettings([string]$espRoot) {
    foreach ($name in $EspSettingsNames) {
        $path = Join-Path $espRoot $name
        if (Test-Path -LiteralPath $path) {
            $null = Assert-Target
            Remove-Item -LiteralPath $path -Force -ErrorAction Stop
            if (Test-Path -LiteralPath $path) { throw "ESP settings file could not be removed: $name" }
            $name
        }
    }
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
function Install-BootIso([string]$mount) {
    # The LiveOS checks this exact ESP path before it starts a backup.
    $isoName = 'crispawn.iso'
    $isoTarget = Join-Path $mount $isoName
    # Older Builder versions wrote hash-named ISOs that the LiveOS cannot open.
    foreach ($oldIso in @(Get-ChildItem -LiteralPath $mount -File -Force -Filter 'crispawn-*.iso' -ErrorAction Stop)) {
        if ($oldIso.Name -cmatch '^crispawn-[0-9a-f]{12}\.iso$') {
            $null = Assert-Target
            Remove-Item -LiteralPath $oldIso.FullName -Force -ErrorAction Stop
        }
    }
    $currentIsoMatches = (Test-Path -LiteralPath $isoTarget) -and
        ((FileHash $isoTarget) -ceq $request.iso_sha256)
    if (!$currentIsoMatches) {
        $null = Assert-Target
        $pendingIso = Join-Path $mount 'crispawn-install-pending.iso'
        if (Test-Path -LiteralPath $pendingIso) {
            Remove-Item -LiteralPath $pendingIso -Force -ErrorAction Stop
        }
        Copy-IsoWithProgress $request.iso $pendingIso
        if ((FileHash $pendingIso) -cne $request.iso_sha256) {
            throw 'Staged ISO media readback mismatch.'
        }
        $null = Assert-Target
        if (Test-Path -LiteralPath $isoTarget) {
            Remove-Item -LiteralPath $isoTarget -Force -ErrorAction Stop
        }
        Move-Item -LiteralPath $pendingIso -Destination $isoTarget -ErrorAction Stop
    }
    if ((FileHash $isoTarget) -cne $request.iso_sha256) {
        throw 'ISO media readback mismatch.'
    }
    $oldBootIso = Join-Path $mount 'perfect-backup.iso'
    if (Test-Path -LiteralPath $oldBootIso) {
        $null = Assert-Target
        Remove-Item -LiteralPath $oldBootIso -Force -ErrorAction Stop
    }
    return $isoName
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
$disk = if ($initialize) { Assert-Identity $true } else { Assert-Target }
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
if (!$initialize -and (Get-DataVolumeDirty)) {
    throw 'USB_NTFS_DIRTY: The existing backup volume needs repair or explicit reinitialization before writing.'
}
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
    if ($initialize) {
        Initialize-SelectedUsb
    }
    $disk = Assert-Target
    $esp = Get-Partition -DiskNumber $disk.Number -PartitionNumber $script:targetEsp.PartitionNumber
    $data = Get-Partition -DiskNumber $disk.Number -PartitionNumber $script:targetData.PartitionNumber
    $fat = Read-Sector $esp.Offset
    $ntfs = Read-Sector $data.Offset
    if ([Text.Encoding]::ASCII.GetString($fat,82,8) -cne 'FAT32   ' -or
        [Text.Encoding]::ASCII.GetString($ntfs,3,8) -cne 'NTFS    ') {
        throw 'USB boot sector signatures changed.'
    }
    $bootUuid = [BitConverter]::ToUInt32($fat,67).ToString('X8').Insert(4,'-')
    $dataUuid = [BitConverter]::ToUInt64($ntfs,72).ToString('X16')
    Add-PartitionAccessPath -DiskNumber (Assert-Target).Number -PartitionNumber $script:targetData.PartitionNumber -AccessPath $dataMount
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
    Add-PartitionAccessPath -DiskNumber (Assert-Target).Number -PartitionNumber $script:targetEsp.PartitionNumber -AccessPath $mount
    $mounted = $true
    Report-Progress 'writingIso' 25
    $isoName = Install-BootIso $mount
    $isoTarget = Join-Path $mount $isoName
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
    Report-Progress 'verifyingFinal' 95
    if ((FileHash $isoTarget) -cne $request.iso_sha256) { throw 'Final ISO readback mismatch.' }
    # A settings file on the ESP would silently override the settings embedded in this ISO.
    $removedEspSettings = @(Remove-EspSettings $mount)
    if (Test-Path -LiteralPath $legacyDirectory) {
        $null = Assert-Target
        Move-Item -LiteralPath $legacyDirectory -Destination $crispawnDirectory -ErrorAction Stop
    } elseif (!(Test-Path -LiteralPath $crispawnDirectory)) {
        $null = Assert-Target
        $null = New-Item -ItemType Directory -Path $crispawnDirectory -ErrorAction Stop
    }
    $dataVolume = Get-Volume -Partition (Get-Partition -DiskNumber (Assert-Target).Number -PartitionNumber $script:targetData.PartitionNumber)
    if ($dataVolume.FileSystemLabel -cne 'CRISPAWN') {
        Set-Volume -InputObject $dataVolume -NewFileSystemLabel CRISPAWN
    }
    if ((Get-Volume -Partition (Get-Partition -DiskNumber (Assert-Target).Number -PartitionNumber $script:targetData.PartitionNumber)).FileSystemLabel -cne 'CRISPAWN' -or
        !(Test-Path -LiteralPath $crispawnDirectory -PathType Container)) {
        throw 'CRISPAWN data volume rename could not be verified.'
    }
    $finalResult = [ordered]@{
        schema_version = 1
        disk_number = [int]$request.disk_number
        disk_unique_id = [string]$request.disk_unique_id
        disk_bytes = [UInt64]$request.disk_bytes
        boot_uuid = $bootUuid
        data_uuid = $dataUuid
        iso_name = $isoName
        iso_sha256 = [string]$request.iso_sha256
        removed_esp_settings = $removedEspSettings
        network_profile = 'none'
        ntfs_preserved = !$initialize
        data_volume_label = 'CRISPAWN'
        data_directory = 'crispawn'
        readback_verified = $true
        ntfs_dirty_verified = $true
    }
} catch {
    [IO.File]::WriteAllText((Join-Path $reportDir 'failure.txt'),
        $_.Exception.Message, [Text.UTF8Encoding]::new($false))
    throw
} finally {
    try {
        if ($dataMounted) {
            Remove-PartitionAccessPath -DiskNumber (Assert-Target).Number -PartitionNumber $script:targetData.PartitionNumber -AccessPath $dataMount
        }
    } finally {
        if ($mounted) {
            Remove-PartitionAccessPath -DiskNumber (Assert-Target).Number -PartitionNumber $script:targetEsp.PartitionNumber -AccessPath $mount
        }
    }
}
if (!(Wait-DataVolumeClean)) {
    if (!$initialize) {
        throw 'USB_NTFS_DIRTY: The backup volume became dirty during writing. Repair it in Windows before use.'
    }
    # Only a newly formatted, empty data volume may be repaired automatically.
    $null = Assert-Target
    $newData = Get-Partition -DiskNumber $request.disk_number -PartitionNumber $script:targetData.PartitionNumber -ErrorAction Stop
    $newVolume = Get-Volume -Partition $newData -ErrorAction Stop
    $null = Repair-Volume -InputObject $newVolume -OfflineScanAndFix -ErrorAction Stop
    if (!(Wait-DataVolumeClean)) {
        throw 'USB_NTFS_DIRTY: Newly formatted data volume is still dirty after repair.'
    }
}
[IO.File]::WriteAllText($resultPath, ($finalResult | ConvertTo-Json -Depth 4), [Text.UTF8Encoding]::new($false))
Report-Progress 'verifyingFinal' 99
} catch {
    $failure = $_.Exception.Message
    try {
        $failurePath = Join-Path ([IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($RequestPath))) 'failure.txt'
        if (!(Test-Path -LiteralPath $failurePath)) {
            [IO.File]::WriteAllText($failurePath, $failure, [Text.UTF8Encoding]::new($false))
        }
    } catch { }
    throw
}
