// Copyright (c) 2026 bunbun-digital and contributors. SPDX-License-Identifier: Apache-2.0
//! Install a verified ISO on an already prepared CRISPAWN USB disk.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::image::Writer;

const HELPER: &str = include_str!("usb_writer.ps1");

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedUsbDisk {
    pub disk_number: u32,
    pub friendly_name: String,
    pub unique_id: String,
    pub size_bytes: u64,
    pub drive_letter: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InternalDisk {
    pub disk_number: u32,
    pub friendly_name: String,
    pub size_bytes: u64,
    pub bus_type: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UsbProgress {
    pub phase: String,
    pub percent: u8,
}

pub fn list_internal() -> Result<Vec<InternalDisk>, String> {
    if !cfg!(windows) {
        return Err("Disk enumeration requires Windows".into());
    }
    let script = r#"$items = @(Get-Disk | Where-Object { $_.BusType -in @('NVMe','SATA','SAS','RAID','ATA','SCSI') -and -not $_.IsOffline } | ForEach-Object { [pscustomobject]@{ diskNumber = [int]$_.Number; friendlyName = [string]$_.FriendlyName; sizeBytes = [UInt64]$_.Size; busType = [string]$_.BusType } }); ConvertTo-Json -InputObject $items -Compress"#;
    let result = Command::new(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .map_err(|e| e.to_string())?;
    if !result.status.success() {
        return Err("Could not enumerate internal disks".into());
    }
    serde_json::from_slice(&result.stdout).map_err(|_| "Invalid internal disk inventory".into())
}

pub fn list_prepared() -> Result<Vec<PreparedUsbDisk>, String> {
    if !cfg!(windows) {
        return Err("USB enumeration requires Windows".into());
    }
    // Fixed, read-only query. Each returned identity is checked again by the
    // elevated writer immediately before touching a selected device.
    let script = r#"$items = @(Get-Disk | Where-Object { $_.BusType -eq 'USB' -and -not $_.IsBoot -and -not $_.IsSystem -and $_.PartitionStyle -eq 'GPT' } | ForEach-Object { $disk = $_; $parts = @(Get-Partition -DiskNumber $disk.Number); if ($parts.Count -eq 2 -and $parts[0].GptType -eq '{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}' -and $parts[0].Size -eq 256MB -and $parts[1].GptType -eq '{ebd0a0a2-b9e5-4433-87c0-68b6b72699c7}') { $volume = Get-Volume -Partition $parts[1]; if ($volume.FileSystem -eq 'NTFS' -and $volume.FileSystemLabel -eq 'CRISPAWN') { $letter = if ($volume.DriveLetter) { [string]$volume.DriveLetter + ':' } else { $null }; [pscustomobject]@{ diskNumber = [int]$disk.Number; friendlyName = [string]$disk.FriendlyName; uniqueId = [string]$disk.UniqueId; sizeBytes = [UInt64]$disk.Size; driveLetter = $letter } } } }); ConvertTo-Json -InputObject $items -Compress"#;
    let result = Command::new(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .map_err(|e| e.to_string())?;
    if !result.status.success() {
        return Err("Could not enumerate prepared USB disks".into());
    }
    serde_json::from_slice(&result.stdout).map_err(|_| "Invalid USB inventory".into())
}

fn ordinary_windows_path(path: &Path) -> Result<std::path::PathBuf, String> {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\") {
        if rest.starts_with("UNC\\") {
            return Err("Copy the ISO and EFI bundle to a local drive first".into());
        }
        return Ok(std::path::PathBuf::from(rest));
    }
    Ok(path.to_path_buf())
}

pub fn install_prepared(
    iso: &Path,
    efi_dir: &Path,
    expected_iso: &str,
    disk_number: u32,
    disk_unique_id: &str,
    disk_bytes: u64,
    report_dir: &Path,
    preflight: bool,
) -> Result<(), String> {
    install_prepared_with_progress(
        iso,
        efi_dir,
        expected_iso,
        disk_number,
        disk_unique_id,
        disk_bytes,
        report_dir,
        preflight,
        |_| {},
    )
}

pub fn install_prepared_with_progress(
    iso: &Path,
    efi_dir: &Path,
    expected_iso: &str,
    disk_number: u32,
    disk_unique_id: &str,
    disk_bytes: u64,
    report_dir: &Path,
    preflight: bool,
    mut on_progress: impl FnMut(UsbProgress),
) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("USB installation requires Windows".into());
    }
    if disk_number == 0 || disk_unique_id.is_empty() || disk_bytes < 1_000_000_000 {
        return Err("Specify the selected removable disk identity and size".into());
    }
    if expected_iso.len() != 64 || !expected_iso.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("ISO SHA-256 must be 64 hexadecimal characters".into());
    }
    let iso = ordinary_windows_path(&iso.canonicalize().map_err(|e| e.to_string())?)?;
    let efi_dir = ordinary_windows_path(&efi_dir.canonicalize().map_err(|e| e.to_string())?)?;
    if !efi_dir.is_dir() || !Writer::base_digest(&iso)?.eq_ignore_ascii_case(expected_iso) {
        return Err("ISO or SHA-256 mismatch".into());
    }
    let boot: Value =
        serde_json::from_slice(&fs::read(efi_dir.join("boot.json")).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if boot["kind"] != "crispawn.development_efi_files"
        || boot["iso_sha256"]
            .as_str()
            .is_none_or(|hash| !hash.eq_ignore_ascii_case(expected_iso))
        || boot["network_driver_profile"] != "none"
        || boot["network_hardware_drivers"] != false
    {
        return Err("EFI bundle does not match the Wi-Fi-free ISO".into());
    }
    let mut efi = serde_json::Map::new();
    for name in ["shimx64.efi", "grubx64.efi", "mmx64.efi"] {
        let expected = boot["files"][name]["sha256"]
            .as_str()
            .ok_or("EFI manifest is incomplete")?;
        let path = efi_dir.join(name);
        let actual = Writer::base_digest(&path)?;
        if !actual.eq_ignore_ascii_case(expected)
            || fs::metadata(&path).map_err(|e| e.to_string())?.len()
                != boot["files"][name]["bytes"]
                    .as_u64()
                    .ok_or("EFI file size missing")?
        {
            return Err(format!("EFI digest or size mismatch: {name}"));
        }
        efi.insert(name.into(), json!(actual));
    }
    if report_dir.exists() {
        return Err("Choose a new report directory".into());
    }
    fs::create_dir_all(report_dir).map_err(|e| e.to_string())?;
    let report_dir = ordinary_windows_path(&report_dir.canonicalize().map_err(|e| e.to_string())?)?;
    let request = json!({
        "iso": iso,
        "efi_dir": efi_dir,
        "iso_sha256": expected_iso.to_ascii_lowercase(),
        "efi_sha256": efi,
        "disk_number": disk_number,
        "disk_unique_id": disk_unique_id,
        "disk_bytes": disk_bytes,
        "report_dir": report_dir,
    });
    let request_path = report_dir.join("request.json");
    let helper_path = report_dir.join("usb_writer.ps1");
    fs::write(
        &request_path,
        serde_json::to_vec_pretty(&request).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::write(&helper_path, HELPER).map_err(|e| e.to_string())?;
    let mut command = Command::new(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe");
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(helper_path)
        .arg("-RequestPath")
        .arg(request_path);
    if preflight {
        command.arg("-Preflight");
    }
    let progress_path = report_dir.join("progress.json");
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let mut latest = None;
    let status = loop {
        if let Some(progress) = read_progress(&progress_path) {
            if latest.as_ref() != Some(&progress) {
                on_progress(progress.clone());
                latest = Some(progress);
            }
        }
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        thread::sleep(Duration::from_millis(150));
    };
    if let Some(progress) = read_progress(&progress_path) {
        if latest.as_ref() != Some(&progress) {
            on_progress(progress);
        }
    }
    if !status.success() {
        return Err(format!("USB writer exited with {status}"));
    }
    if preflight {
        println!("USB preflight passed: {}", report_dir.display());
        return Ok(());
    }
    let result = report_dir.join("result.json");
    let value: Value = serde_json::from_slice(&fs::read(&result).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if value["iso_sha256"] != expected_iso.to_ascii_lowercase()
        || value["disk_unique_id"] != disk_unique_id
        || value["data_volume_label"] != "CRISPAWN"
        || value["data_directory"] != "crispawn"
        || value["readback_verified"] != true
    {
        return Err("USB result verification failed".into());
    }
    println!("USB installed and read back: {}", result.display());
    Ok(())
}

fn read_progress(path: &Path) -> Option<UsbProgress> {
    let progress: UsbProgress = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    if progress.percent <= 100
        && matches!(
            progress.phase.as_str(),
            "preparing"
                | "awaitingApproval"
                | "writingIso"
                | "verifyingIso"
                | "writingBoot"
                | "verifyingFinal"
                | "complete"
        )
    {
        Some(progress)
    } else {
        None
    }
}
