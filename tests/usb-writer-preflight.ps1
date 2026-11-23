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
function Get-Disk { [CmdletBinding()] param([int]$Number) return $script:mockDisk }
function Get-Partition { [CmdletBinding()] param([int]$DiskNumber) return $script:mockParts }
function Get-Volume { [CmdletBinding()] param($Partition) return [pscustomobject]@{ FileSystem = 'NTFS'; FileSystemLabel = 'CRISPAWN' } }
function Clear-Disk { throw 'A physical disk command was reached in preflight.' }
function Initialize-Disk { throw 'A physical disk command was reached in preflight.' }
function New-Partition { throw 'A physical disk command was reached in preflight.' }
function Format-Volume { throw 'A physical disk command was reached in preflight.' }
$helper = Join-Path $PSScriptRoot '..\crates\builder-core\src\usb_writer.ps1'
$result = . $helper -RequestPath $requestPath -Preflight
if ($result -ne 'USB_PREFLIGHT_OK') { throw 'Raw USB preflight failed.' }
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
$script:mockDisk.SerialNumber = 'CHANGED'
try {
    $null = . $helper -RequestPath $requestPath -Preflight
    throw 'Changed USB identity was accepted.'
} catch {
    if ($_.Exception.Message -eq 'Changed USB identity was accepted.') { throw }
}
Write-Output 'USB writer preflight mock passed.'
