# Copyright (c) 2026 bunbun-digital and contributors. SPDX-License-Identifier: Apache-2.0
[CmdletBinding()] param([switch]$Preflight)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$env:PSModulePath = "$PSHOME\Modules"
$env:Path = "$env:SystemRoot\System32;$env:SystemRoot;$PSHOME"
$env:TEMP = $env:TMP = Join-Path $PSScriptRoot 'temp'

# The submitting account may write this one file. Open the directory entry
# itself and verify its handle before reading any bytes with elevated rights.
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class CrispawnRequestFile {
    [StructLayout(LayoutKind.Sequential)]
    private struct Info {
        public uint Attributes;
        public System.Runtime.InteropServices.ComTypes.FILETIME Created, Accessed, Written;
        public uint Volume, SizeHigh, SizeLow, Links, IndexHigh, IndexLow;
    }
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    private static extern SafeFileHandle CreateFileW(string path, uint access, uint sharing,
        IntPtr security, uint disposition, uint flags, IntPtr template);
    [DllImport("kernel32.dll", SetLastError=true)]
    private static extern bool GetFileInformationByHandle(SafeFileHandle handle, out Info info);
    public static FileStream Open(string path) {
        var handle = CreateFileW(path, 0x80000000, 0, IntPtr.Zero, 3, 0x00200000, IntPtr.Zero);
        if (handle.IsInvalid) throw new Win32Exception(Marshal.GetLastWin32Error());
        try {
            Info info;
            if (!GetFileInformationByHandle(handle, out info)) throw new Win32Exception(Marshal.GetLastWin32Error());
            if ((info.Attributes & (0x400u | 0x10u)) != 0 || info.Links != 1)
                throw new IOException("Expected a single regular submission file.");
            return new FileStream(handle, FileAccess.Read);
        } catch { handle.Dispose(); throw; }
    }
}
'@
$stream = [CrispawnRequestFile]::Open((Join-Path $PSScriptRoot 'request.bin'))
$reader = [IO.BinaryReader]::new($stream)
$job = $null
$passed = $false
try {
    if ([Text.Encoding]::ASCII.GetString($reader.ReadBytes(8)) -cne 'CRISO002') { throw 'Invalid submission format.' }
    $id = [Guid]::new($reader.ReadBytes(16)).ToString('N')
    $hash = [BitConverter]::ToString($reader.ReadBytes(32)).Replace('-','')
    $languageCode = $reader.ReadByte()
    if ($languageCode -gt 1) { throw 'Unknown language code.' }
    $language = @('ja','en')[$languageCode]
    $size = $reader.ReadUInt32()
    if ($size -lt 1 -or $size -ge 100000000 -or ($stream.Length - $stream.Position) -ne $size) {
        throw 'Invalid ISO length.'
    }
    $job = New-Item -ItemType Directory -Path (Join-Path $PSScriptRoot "runs\$id") -ErrorAction Stop
    $iso = Join-Path $job.FullName 'configured.iso'
    try {
        $target = [IO.File]::Open($iso, 'CreateNew', 'Write', 'None')
        try { $stream.CopyTo($target) } finally { $target.Dispose() }
        if ((Get-FileHash -LiteralPath $iso).Hash -cne $hash) { throw 'Submission SHA-256 mismatch.' }
        foreach ($name in @('check-original-BOOTX64.EFI','check-original-grubx64.efi','check-original-mmx64.efi','check-original-grub.cfg')) {
            Copy-Item -LiteralPath (Join-Path $PSScriptRoot $name) -Destination $job.FullName
        }
        & (Join-Path $PSScriptRoot 'New-SecureBootTestMedia.ps1') -InputDirectory $job.FullName -IsoSha256 $hash -Language $language -Preflight:$Preflight -BootTest:(!$Preflight)
        $passed = $true
    } finally {
        $result = @{ id=$id; iso_sha256=$hash; language=$language; preflight=[bool]$Preflight; passed=$passed }
        $temp = Join-Path $job.FullName 'submission-result.tmp'
        $result | ConvertTo-Json | Set-Content -LiteralPath $temp -Encoding UTF8
        [IO.File]::Move($temp,(Join-Path $job.FullName 'submission-result.json'))
    }
} finally { $reader.Dispose() }
