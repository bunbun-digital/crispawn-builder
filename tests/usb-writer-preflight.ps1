# Copyright (c) 2026 bunbun-digital and contributors. SPDX-License-Identifier: Apache-2.0
# Mock the storage cmdlets. This test must never access or change a physical disk.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$root = Join-Path $PSScriptRoot ('..\build\usb-preflight-' + [guid]::NewGuid().ToString('N'))
$null = New-Item -ItemType Directory -Path $root -Force
$efi = Join-Path $root 'efi'
$null = New-Item -ItemType Directory -Path $efi
$iso = Join-Path $root 'base.iso'
[IO.File]::WriteAllBytes($iso, [byte[]](1,2,3))
$hash = (Get-FileHash -LiteralPath $iso -Algorithm SHA256).Hash.ToLowerInvariant()
$efiHashes = @{}
foreach ($name in @('shimx64.efi','grubx64.efi','mmx64.efi')) {
    $path = Join-Path $efi $name
    [IO.File]::WriteAllBytes($path, [Text.Encoding]::ASCII.GetBytes($name))
    $efiHashes[$name] = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
}
$report = Join-Path $root 'report'
$null = New-Item -ItemType Directory -Path $report
$config = Join-Path $report 'live-config.json'
[IO.File]::WriteAllText($config, '{}')
$request = [ordered]@{
    iso = $iso; efi_dir = $efi; iso_sha256 = $hash; efi_sha256 = $efiHashes
    disk_number = 4; disk_unique_id = 'USB-ID'; disk_bytes = [UInt64]64000000000
    report_dir = $report; config_path = $config
    config_sha256 = (Get-FileHash -LiteralPath $config -Algorithm SHA256).Hash.ToLowerInvariant()
    disk_path = 'USB-PATH'; serial_number = 'USB-SERIAL'; layout_stamp = 'RAW|'; initialize = $true
}
$requestPath = Join-Path $root 'request.json'
$request | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $requestPath -Encoding UTF8
$script:mockDisk = [pscustomobject]@{
    Number = 4; UniqueId = 'USB-ID'; Path = 'USB-PATH'; SerialNumber = 'USB-SERIAL'
    Size = [UInt64]64000000000; BusType = 'USB'; PartitionStyle = 'RAW'
    IsBoot = $false; IsSystem = $false; IsOffline = $false; IsReadOnly = $false
}
$script:mockParts = @()
$script:mockDirty = $false
$script:calls = [System.Collections.Generic.List[string]]::new()
function Get-Disk { [CmdletBinding()] param([int]$Number) return $script:mockDisk }
function Get-Partition {
    [CmdletBinding()] param([int]$DiskNumber,[int]$PartitionNumber)
    if ($PSBoundParameters.ContainsKey('PartitionNumber')) {
        return $script:mockParts | Where-Object PartitionNumber -eq $PartitionNumber
    }
    return $script:mockParts
}
function Get-Volume { [CmdletBinding()] param($Partition) return [pscustomobject]@{
    FileSystem = 'NTFS'; FileSystemLabel = 'CRISPAWN'; Path = '\\?\Volume{test-usb}\'
} }
function Get-CimInstance { [CmdletBinding()] param([string]$ClassName) return [pscustomobject]@{
    DeviceID = '\\?\Volume{test-usb}\'; DirtyBitSet = $script:mockDirty
} }
function Clear-Disk {
    [CmdletBinding(SupportsShouldProcess=$true)] param([int]$Number,[switch]$RemoveData,[switch]$RemoveOEM)
    $script:calls.Add('clear')
    $script:mockParts = @()
}
function Initialize-Disk {
    [CmdletBinding()] param([int]$Number,[string]$PartitionStyle)
    if ($script:mockDisk.PartitionStyle -ne 'RAW') { throw 'Mock disk is already initialized.' }
    $script:calls.Add('initialize')
    $script:mockDisk.PartitionStyle = $PartitionStyle
}
function Set-Disk {
    [CmdletBinding()] param([int]$Number,[string]$PartitionStyle)
    if ($script:mockDisk.PartitionStyle -ne 'MBR' -or $script:mockParts.Count -ne 0) { throw 'Mock disk cannot be converted.' }
    $script:calls.Add('convert')
    $script:mockDisk.PartitionStyle = $PartitionStyle
}
function New-Partition {
    [CmdletBinding()] param([int]$DiskNumber,[UInt64]$Size,[switch]$UseMaximumSize,[string]$GptType)
    $script:calls.Add('partition')
    $number = $script:mockParts.Count + 1
    $part = [pscustomobject]@{
        PartitionNumber = $number
        Offset = if ($number -eq 1) { [UInt64]1048576 } else { [UInt64]269484032 }
        Size = if ($number -eq 1) { $Size } else { [UInt64]63000000000 }
        GptType = $GptType; MbrType = ''
    }
    $script:mockParts = @($script:mockParts) + @($part)
    return $part
}
function Format-Volume {
    [CmdletBinding(SupportsShouldProcess=$true)] param($Partition,[string]$FileSystem,[string]$NewFileSystemLabel)
    $script:calls.Add('format')
}
$helper = Join-Path $PSScriptRoot '..\crates\builder-core\src\usb_writer.ps1'
$result = . $helper -RequestPath $requestPath -Preflight
if ($result -ne 'USB_PREFLIGHT_OK') { throw 'Raw USB preflight failed.' }
if ($script:calls.Count -ne 0) { throw 'Raw preflight mutated a disk.' }
$script:mockDisk.PartitionStyle = 'GPT'
$esp = [pscustomobject]@{
    PartitionNumber = 1; Offset = [UInt64]1048576; Size = [UInt64]268435456
    GptType = '{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}'; MbrType = ''
}
$script:mockParts = @($esp)
$request.layout_stamp = 'GPT|1:1048576:268435456:{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}:'
$request | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $requestPath -Encoding UTF8
$result = . $helper -RequestPath $requestPath -Preflight
if ($result -ne 'USB_PREFLIGHT_OK') { throw 'Partially initialized USB preflight failed.' }
if ($script:calls.Count -ne 0) { throw 'GPT preflight mutated a disk.' }
Initialize-SelectedUsb
if ($script:calls.Contains('initialize') -or $script:calls.Contains('convert') -or !$script:calls.Contains('clear')) {
    throw 'Partially initialized GPT disk used the wrong initialization path.'
}
$null = Assert-Target
$script:calls.Clear()
$script:mockParts = @()
$request.layout_stamp = 'GPT|'
$request | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $requestPath -Encoding UTF8
$result = . $helper -RequestPath $requestPath -Preflight
if ($result -ne 'USB_PREFLIGHT_OK') { throw 'Empty GPT USB preflight failed.' }
Initialize-SelectedUsb
if ($script:calls.Contains('clear') -or $script:calls.Contains('initialize') -or $script:calls.Contains('convert')) {
    throw 'Empty GPT disk was unnecessarily initialized.'
}
$null = Assert-Target
$script:calls.Clear()
$script:mockParts = @()
$script:mockDisk.PartitionStyle = 'RAW'
$request.layout_stamp = 'RAW|'
$request | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $requestPath -Encoding UTF8
$result = . $helper -RequestPath $requestPath -Preflight
if ($result -ne 'USB_PREFLIGHT_OK') { throw 'Raw USB second preflight failed.' }
Initialize-SelectedUsb
if (!$script:calls.Contains('initialize') -or $script:calls.Contains('clear')) { throw 'RAW disk initialization path failed.' }
$null = Assert-Target
$script:calls.Clear()
$script:mockParts = @()
$script:mockDisk.PartitionStyle = 'MBR'
$request.layout_stamp = 'MBR|'
$request | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $requestPath -Encoding UTF8
$result = . $helper -RequestPath $requestPath -Preflight
if ($result -ne 'USB_PREFLIGHT_OK') { throw 'MBR USB preflight failed.' }
Initialize-SelectedUsb
if (!$script:calls.Contains('convert') -or $script:calls.Contains('initialize')) { throw 'MBR conversion path failed.' }
$null = Assert-Target
$script:calls.Clear()
$script:mockDisk.PartitionStyle = 'GPT'
$data = [pscustomobject]@{
    PartitionNumber = 2; Offset = [UInt64]269484032; Size = [UInt64]63000000000
    GptType = '{ebd0a0a2-b9e5-4433-87c0-68b6b72699c7}'; MbrType = ''
}
$script:mockParts = @($esp,$data)
$request.initialize = $false
$request.layout_stamp = 'GPT|1:1048576:268435456:{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}:|2:269484032:63000000000:{ebd0a0a2-b9e5-4433-87c0-68b6b72699c7}:'
$request | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $requestPath -Encoding UTF8
$result = . $helper -RequestPath $requestPath -Preflight
if ($result -ne 'USB_PREFLIGHT_OK') { throw 'Prepared USB preflight failed.' }
if (Get-DataVolumeDirty) { throw 'Clean USB was reported dirty.' }
$script:mockDirty = $true
if (!(Get-DataVolumeDirty)) { throw 'Dirty USB was reported clean.' }
$script:mockDirty = $null
try {
    $null = Get-DataVolumeDirty
    throw 'Unknown NTFS dirty state was accepted.'
} catch {
    if ($_.Exception.Message -eq 'Unknown NTFS dirty state was accepted.') { throw }
}
$script:mockDirty = $false
$request.initialize = $true
$request | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $requestPath -Encoding UTF8
$result = . $helper -RequestPath $requestPath -Preflight
if ($result -ne 'USB_PREFLIGHT_OK') { throw 'Prepared USB reinitialization preflight failed.' }
$request.initialize = $false
$request | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $requestPath -Encoding UTF8
$script:mockDisk.SerialNumber = 'CHANGED'
try {
    $null = . $helper -RequestPath $requestPath -Preflight
    throw 'Changed USB identity was accepted.'
} catch {
    if ($_.Exception.Message -eq 'Changed USB identity was accepted.') { throw }
}
$script:mockDisk.SerialNumber = 'USB-SERIAL'
$mount = Join-Path $root 'esp'
$null = New-Item -ItemType Directory -Path $mount
$legacyIso = Join-Path $mount ('crispawn-' + $hash.Substring(0,12) + '.iso')
Copy-Item -LiteralPath $iso -Destination $legacyIso
[IO.File]::WriteAllBytes((Join-Path $mount 'perfect-backup.iso'), [byte[]](9,8,7))
if ((Install-BootIso $mount) -cne 'crispawn.iso' -or
    (FileHash (Join-Path $mount 'crispawn.iso')) -cne $hash -or
    (Test-Path -LiteralPath $legacyIso) -or
    (Test-Path -LiteralPath (Join-Path $mount 'perfect-backup.iso')) -or
    (Test-Path -LiteralPath (Join-Path $mount 'crispawn-install-pending.iso'))) {
    throw 'Boot ISO replacement did not match the LiveOS path.'
}
$updatedIso = Join-Path $root 'updated.iso'
[IO.File]::WriteAllBytes($updatedIso, [byte[]](4,5,6,7))
$request.iso = $updatedIso
$request.iso_sha256 = (Get-FileHash -LiteralPath $updatedIso -Algorithm SHA256).Hash.ToLowerInvariant()
$null = Install-BootIso $mount
if ((FileHash (Join-Path $mount 'crispawn.iso')) -cne $request.iso_sha256) {
    throw 'Boot ISO update did not replace the previous version.'
}
Write-Output 'USB writer preflight mock passed.'
