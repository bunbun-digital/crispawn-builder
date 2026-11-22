# Copyright (c) 2026 bunbun-digital and contributors. SPDX-License-Identifier: Apache-2.0
# Windows-native virtual media writer for the verified 2026-11-21 development profile.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$InputDirectory,
    [ValidatePattern('^[0-9a-fA-F]{64}$')][string]$IsoSha256 = 'c126dea78dd214f55916d04f4435b684320a323f81f43667f0514eb3a4e601b4',
    [ValidateSet('ja','en')][string]$Language = 'en',
    [switch]$BootTest,
    [switch]$Preflight
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$inputRoot = (Get-Item -LiteralPath $InputDirectory).FullName
$expected = [ordered]@{
    'configured.iso' = $IsoSha256
    'check-original-BOOTX64.EFI' = '4c89145e958cf592a6f16552eadf112ef2c1c525e2435c2761e6a99fa88188b3'
    'check-original-grubx64.efi' = '33e1d46cb68e41a293c767750078bbdb857ad7c1197a2206dea737bc9ccb8dbf'
    'check-original-mmx64.efi' = '5f9fc41db3dfe3b581fc20cae696af82c5ca21e96071ec9c4c28ee4a15f989d8'
    'check-original-grub.cfg' = '01056b6cfed22e4d033018b09f2501925b3250a336c245e46683b49aafcb0642'
}
foreach ($entry in $expected.GetEnumerator()) {
    if ((Get-FileHash -LiteralPath (Join-Path $inputRoot $entry.Key)).Hash -ine $entry.Value) {
        throw "Development profile hash mismatch: $($entry.Key)"
    }
}
if ($Preflight) { return }

$run = New-Item -ItemType Directory -Path (Join-Path $inputRoot ('fresh-media-' + [Guid]::NewGuid().ToString('N')))
trap {
    $_.Exception.Message | Set-Content -LiteralPath (Join-Path $run.FullName 'failure.txt')
    break
}
$vhd = Join-Path $run.FullName 'boot.vhdx'
function Get-OwnedDisk {
    $attachment = Get-VHD -Path $vhd
    if (!$attachment.Attached -or $attachment.Path -ine $vhd -or $null -eq $attachment.DiskNumber) {
        throw 'New VHDX attachment identity changed.'
    }
    $disk = Get-Disk -Number $attachment.DiskNumber
    if ($disk.IsSystem -or $disk.IsBoot -or $disk.Size -ne 2GB -or
        [int]$disk.CimInstanceProperties['BusType'].Value -ne 15) { throw 'Unexpected disk identity.' }
    return $disk
}
function Read-BootSector($partition) {
    $disk = Get-OwnedDisk
    $stream = [IO.File]::Open(("\\.\PhysicalDrive" + $disk.Number), 'Open', 'Read', 'ReadWrite')
    try {
        $null = $stream.Seek($partition.Offset, 'Begin')
        $sector = New-Object byte[] 512
        if ($stream.Read($sector, 0, 512) -ne 512) { throw 'Short filesystem boot sector.' }
        return ,$sector
    } finally { $stream.Dispose() }
}
New-VHD -Path $vhd -Dynamic -SizeBytes 2GB -LogicalSectorSizeBytes 512 | Out-Null
Mount-VHD -Path $vhd -NoDriveLetter
try {
    $disk = Get-OwnedDisk
    if ($disk.PartitionStyle -ne 'RAW') { throw 'Expected a blank virtual disk.' }
    if ($disk.IsOffline) { Set-Disk -InputObject $disk -IsOffline $false }
    Initialize-Disk -InputObject $disk -PartitionStyle GPT
    $esp = New-Partition -DiskNumber (Get-OwnedDisk).Number -Size 128MB -Alignment 1MB -GptType '{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}'
    $data = New-Partition -DiskNumber (Get-OwnedDisk).Number -UseMaximumSize -Alignment 1MB -GptType '{ebd0a0a2-b9e5-4433-87c0-68b6b72699c7}'
    foreach ($part in @($esp,$data)) {
        if ($part.DiskNumber -ne (Get-OwnedDisk).Number) { throw 'Partition identity changed.' }
        if ($part.PartitionNumber -eq $esp.PartitionNumber) {
            Format-Volume -Partition $part -FileSystem FAT32 -NewFileSystemLabel CRISPAWN -Confirm:$false | Out-Null
        } else {
            Format-Volume -Partition $part -FileSystem NTFS -NewFileSystemLabel CRISPAWN -Confirm:$false | Out-Null
        }
    }
    $fat = Read-BootSector $esp
    $ntfs = Read-BootSector $data
    if ([Text.Encoding]::ASCII.GetString($fat,82,8) -ne 'FAT32   ' -or
        [Text.Encoding]::ASCII.GetString($ntfs,3,8) -ne 'NTFS    ') { throw 'Filesystem signature mismatch.' }
    $fatSerial = [BitConverter]::ToUInt32($fat,67).ToString('X8').Insert(4,'-')
    $ntfsSerial = [BitConverter]::ToUInt64($ntfs,72).ToString('X16')
    $mount = New-Item -ItemType Directory -Path (Join-Path $run.FullName 'esp')
    Add-PartitionAccessPath -DiskNumber (Get-OwnedDisk).Number -PartitionNumber $esp.PartitionNumber -AccessPath $mount.FullName
    $contents = [ordered]@{'perfect-backup.iso'='configured.iso'}
    foreach ($dir in @('EFI\BOOT','EFI\ubuntu','boot\grub')) {
        [IO.Directory]::CreateDirectory((Join-Path $mount.FullName $dir)) | Out-Null
    }
    foreach ($efi in @('BOOTX64.EFI','grubx64.efi','mmx64.efi')) { $contents["EFI\BOOT\$efi"]="check-original-$efi" }
    foreach ($entry in $contents.GetEnumerator()) {
        $dest = Join-Path $mount.FullName $entry.Key
        Copy-Item -LiteralPath (Join-Path $inputRoot $entry.Value) -Destination $dest
        if ((Get-FileHash -LiteralPath $dest).Hash -ine $expected[$entry.Value]) { throw 'Payload readback mismatch.' }
    }
    $template = [IO.File]::ReadAllBytes((Join-Path $inputRoot 'check-original-grub.cfg'))
    $sha = [Security.Cryptography.SHA256]::Create()
    try {
        if ([BitConverter]::ToString($sha.ComputeHash($template)).Replace('-','') -ine $expected['check-original-grub.cfg']) {
            throw 'GRUB configuration changed.'
        }
    } finally { $sha.Dispose() }
    $cfg = [Text.Encoding]::ASCII.GetString($template).Replace('1228-42C3',$fatSerial).Replace('47F59EFC06CB39D3',$ntfsSerial)
    foreach ($dir in @('EFI\BOOT','EFI\ubuntu','boot\grub')) {
        $dest = Join-Path (Join-Path $mount.FullName $dir) 'grub.cfg'
        [IO.File]::WriteAllText($dest,$cfg,[Text.Encoding]::ASCII)
        if ([IO.File]::ReadAllText($dest) -cne $cfg) { throw 'GRUB configuration readback mismatch.' }
    }
} finally { Dismount-VHD -Path $vhd }
$hash = (Get-FileHash -LiteralPath $vhd).Hash
[ordered]@{vhdx_sha256=$hash; boot_uuid=$fatSerial; data_uuid=$ntfsSerial; payload=$expected} |
    ConvertTo-Json | Set-Content -LiteralPath (Join-Path $run.FullName 'media.json')
Write-Output $vhd
if ($BootTest) { & (Join-Path $PSScriptRoot 'Invoke-SecureBootSmoke.ps1') -BootImage $vhd -Sha256 $hash -Language $Language }
