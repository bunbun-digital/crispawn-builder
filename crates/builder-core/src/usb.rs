// Copyright (c) 2026 bunbun-digital and contributors. SPDX-License-Identifier: Apache-2.0
//! Inventory USB bus disks and install a verified ISO on the selected device.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::image::Writer;
use crate::{Project, plan};

const HELPER: &str = include_str!("usb_writer.ps1");
const MIN_USB_BYTES: u64 = 512 * 1024 * 1024;

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
pub struct UsbDisk {
    pub disk_number: u32,
    pub friendly_name: String,
    pub unique_id: String,
    pub disk_path: String,
    pub serial_number: String,
    pub size_bytes: u64,
    pub drive_letter: Option<String>,
    pub layout_stamp: String,
    pub prepared: bool,
    pub eligible: bool,
    pub issue: Option<String>,
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
    Ok(list_usb()?
        .into_iter()
        .filter(|disk| disk.prepared && disk.eligible)
        .map(|disk| PreparedUsbDisk {
            disk_number: disk.disk_number,
            friendly_name: disk.friendly_name,
            unique_id: disk.unique_id,
            size_bytes: disk.size_bytes,
            drive_letter: disk.drive_letter,
        })
        .collect())
}

pub fn list_usb() -> Result<Vec<UsbDisk>, String> {
    if !cfg!(windows) {
        return Err("USB enumeration requires Windows".into());
    }
    // Read-only inventory. BusType USB includes USB HDDs and SSDs as well as flash media.
    // The elevated helper rechecks identity, layout and safety state before mutation.
    let script = r#"
$items = @(Get-Disk | Where-Object { $_.BusType -eq 'USB' } | ForEach-Object {
    $disk = $_
    $parts = @(Get-Partition -DiskNumber $disk.Number -ErrorAction SilentlyContinue | Sort-Object PartitionNumber)
    $layout = [string]$disk.PartitionStyle + '|' + (($parts | ForEach-Object {
        [string]$_.PartitionNumber + ':' + [string]$_.Offset + ':' + [string]$_.Size + ':' + [string]$_.GptType + ':' + [string]$_.MbrType
    }) -join '|')
    $letters = @($parts | ForEach-Object { Get-Volume -Partition $_ -ErrorAction SilentlyContinue } | Where-Object DriveLetter | ForEach-Object { [string]$_.DriveLetter + ':' })
    $prepared = $false
    if ($disk.PartitionStyle -eq 'GPT' -and $parts.Count -eq 2 -and
        $parts[0].PartitionNumber -eq 1 -and $parts[1].PartitionNumber -eq 2 -and
        $parts[0].GptType -ieq '{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}' -and
        $parts[0].Size -eq 256MB -and
        $parts[1].GptType -ieq '{ebd0a0a2-b9e5-4433-87c0-68b6b72699c7}') {
        $volume = Get-Volume -Partition $parts[1] -ErrorAction SilentlyContinue
        $prepared = $volume -and $volume.FileSystem -eq 'NTFS' -and $volume.FileSystemLabel -in @('CRISPAWN','PERFECTBACKUP')
    }
    $issue = $null
    if ($disk.IsBoot -or $disk.IsSystem) { $issue = 'system' }
    elseif ($disk.IsOffline) { $issue = 'offline' }
    elseif ($disk.IsReadOnly) { $issue = 'readOnly' }
    elseif ($disk.Size -lt 536870912) { $issue = 'tooSmall' }
    elseif (!$disk.Path -and !$disk.UniqueId -and !$disk.SerialNumber) { $issue = 'noIdentity' }
    [pscustomobject]@{
        diskNumber = [int]$disk.Number; friendlyName = [string]$disk.FriendlyName
        uniqueId = [string]$disk.UniqueId; diskPath = [string]$disk.Path
        serialNumber = [string]$disk.SerialNumber; sizeBytes = [UInt64]$disk.Size
        driveLetter = if ($letters.Count) { $letters -join ', ' } else { $null }
        layoutStamp = $layout; prepared = [bool]$prepared
        eligible = !$issue; issue = $issue
    }
}); ConvertTo-Json -InputObject $items -Compress
"#;
    let result = Command::new(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .map_err(|e| e.to_string())?;
    if !result.status.success() {
        return Err(format!(
            "Could not enumerate USB disks: {}",
            String::from_utf8_lossy(&result.stderr)
        ));
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
    install_prepared_with_settings(
        iso,
        efi_dir,
        expected_iso,
        disk_number,
        disk_unique_id,
        disk_bytes,
        report_dir,
        preflight,
        None,
        &mut on_progress,
    )
}

pub fn install_prepared_with_settings(
    iso: &Path,
    efi_dir: &Path,
    expected_iso: &str,
    disk_number: u32,
    disk_unique_id: &str,
    disk_bytes: u64,
    report_dir: &Path,
    preflight: bool,
    settings: Option<&Project>,
    on_progress: impl FnMut(UsbProgress),
) -> Result<(), String> {
    install_with_settings(
        iso,
        efi_dir,
        expected_iso,
        disk_number,
        disk_unique_id,
        disk_bytes,
        report_dir,
        preflight,
        settings,
        None,
        on_progress,
    )
}

pub fn install_selected_with_settings(
    iso: &Path,
    efi_dir: &Path,
    expected_iso: &str,
    disk: &UsbDisk,
    report_dir: &Path,
    settings: &Project,
    on_progress: impl FnMut(UsbProgress),
) -> Result<(), String> {
    if !disk.eligible
        || (disk.disk_path.is_empty() && disk.unique_id.is_empty() && disk.serial_number.is_empty())
    {
        return Err("Selected USB has no safe hardware identity or is unavailable".into());
    }
    let current = list_usb()?
        .into_iter()
        .find(|candidate| candidate.disk_number == disk.disk_number)
        .ok_or("Selected USB is no longer connected")?;
    if current.disk_path != disk.disk_path
        || current.unique_id != disk.unique_id
        || current.serial_number != disk.serial_number
        || current.size_bytes != disk.size_bytes
        || current.layout_stamp != disk.layout_stamp
        || current.prepared != disk.prepared
        || !current.eligible
    {
        return Err("Selected USB identity or layout changed. Refresh the USB list.".into());
    }
    install_with_settings(
        iso,
        efi_dir,
        expected_iso,
        disk.disk_number,
        &disk.unique_id,
        disk.size_bytes,
        report_dir,
        false,
        Some(settings),
        Some(disk),
        on_progress,
    )
}

fn install_with_settings(
    iso: &Path,
    efi_dir: &Path,
    expected_iso: &str,
    disk_number: u32,
    disk_unique_id: &str,
    disk_bytes: u64,
    report_dir: &Path,
    preflight: bool,
    settings: Option<&Project>,
    selected: Option<&UsbDisk>,
    mut on_progress: impl FnMut(UsbProgress),
) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("USB installation requires Windows".into());
    }
    if (disk_unique_id.is_empty() && selected.is_none()) || disk_bytes < MIN_USB_BYTES {
        return Err("Specify the selected USB disk identity and size".into());
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
        && boot["kind"] != "crispawn.github_release_efi_files"
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
    let mut request = json!({
        "iso": iso,
        "efi_dir": efi_dir,
        "iso_sha256": expected_iso.to_ascii_lowercase(),
        "efi_sha256": efi,
        "disk_number": disk_number,
        "disk_unique_id": disk_unique_id,
        "disk_bytes": disk_bytes,
        "report_dir": report_dir,
    });
    if let Some(disk) = selected {
        request["disk_path"] = json!(disk.disk_path);
        request["serial_number"] = json!(disk.serial_number);
        request["layout_stamp"] = json!(disk.layout_stamp);
        request["initialize"] = json!(!disk.prepared);
    }
    if let Some(project) = settings {
        let config = plan(project).map_err(str::to_owned)?;
        let path = report_dir.join("live-config.json");
        fs::write(&path, config).map_err(|e| e.to_string())?;
        request["config_path"] = json!(path);
        request["config_sha256"] = json!(Writer::base_digest(&path)?);
    }
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
        || (settings.is_some() && value["config_sha256"] != request["config_sha256"])
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
                | "initializing"
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
