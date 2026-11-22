// Copyright (c) 2026 bunbun-digital and contributors. SPDX-License-Identifier: Apache-2.0
//! Extract pinned signed EFI bytes embedded in the selected ISO.

use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::fat12;
use crate::image::Writer;

const PINS: &str = include_str!("../../../data/debian-base-boot.json");
const KERNEL_LIMIT: u64 = 32 * 1024 * 1024;
const EFI_LIMIT: u64 = 5 * 1024 * 1024;
const FAT_LIMIT: u64 = 32 * 1024 * 1024;
const LAYOUT_LIMIT: u64 = 4096;
const EFI_FILES: [(&str, &str, &[u8; 11]); 3] = [
    ("EFI/BOOT/BOOTX64.EFI", "shimx64.efi", b"BOOTX64 EFI"),
    ("EFI/BOOT/grubx64.efi", "grubx64.efi", b"GRUBX64 EFI"),
    ("EFI/BOOT/mmx64.efi", "mmx64.efi", b"MMX64   EFI"),
];

/// The release metadata pins the Debian-signed bytes, while the selected ISO
/// supplies those bytes. Nothing is fetched or chosen from a separate folder.
pub fn extract_for_iso(
    iso: &Path,
    output: &Path,
    mut progress: impl FnMut(u8),
) -> Result<String, String> {
    let mut boot: Value = serde_json::from_str(PINS).map_err(|e| e.to_string())?;
    if boot["kind"] != "crispawn.development_efi_files" {
        return Err("Invalid built-in EFI release metadata.".into());
    }
    let iso_hash = Writer::base_digest(iso)?;
    let kernel = extract_entry(iso, "boot/bzImage", KERNEL_LIMIT)?;
    if hex::encode(Sha256::digest(&kernel))
        != boot["kernel_sha256"]
            .as_str()
            .ok_or("Missing kernel pin.")?
    {
        return Err("The ISO kernel does not match the signed EFI release.".into());
    }
    let layout = extract_entry(iso, "boot/crispawn-media.json", LAYOUT_LIMIT)
        .map_err(|_| "Select an ISO with the CRISPAWN USB data layout.".to_string())?;
    verify_data_layout(&layout)?;
    progress(2);
    fs::create_dir_all(output).map_err(|e| e.to_string())?;
    let direct_first = extract_entry(iso, EFI_FILES[0].0, EFI_LIMIT);
    let fat_image = match &direct_first {
        Err(error) if error.starts_with("ISO entry is missing or invalid:") => Some(
            extract_entry(iso, "boot/fat.efi", FAT_LIMIT)
                .map_err(|_| "The ISO is missing its signed EFI files.".to_string())?,
        ),
        _ => None,
    };
    for (index, (member, name, fat_name)) in EFI_FILES.iter().enumerate() {
        let data = if let Some(fat_image) = &fat_image {
            fat12::extract_boot_file(fat_image, fat_name, EFI_LIMIT as usize)?
        } else if index == 0 {
            direct_first.as_ref().map_err(Clone::clone)?.clone()
        } else {
            extract_entry(iso, member, EFI_LIMIT)?
        };
        let expected_size = boot["files"][name]["bytes"]
            .as_u64()
            .ok_or("Missing signed EFI size pin.")?;
        let expected_hash = boot["files"][name]["sha256"]
            .as_str()
            .ok_or("Missing signed EFI SHA-256 pin.")?;
        if data.len() as u64 != expected_size || hex::encode(Sha256::digest(&data)) != expected_hash
        {
            return Err(format!(
                "Signed EFI file in ISO does not match the release: {member}"
            ));
        }
        fs::write(output.join(name), data).map_err(|e| e.to_string())?;
        progress(6 + index as u8 * 4);
    }
    boot["iso_sha256"] = json!(iso_hash);
    boot["iso_name"] = json!(
        iso.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("base.iso")
    );
    fs::write(
        output.join("boot.json"),
        serde_json::to_vec_pretty(&boot).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(iso_hash)
}

fn verify_data_layout(raw: &[u8]) -> Result<(), String> {
    let layout: Value =
        serde_json::from_slice(raw).map_err(|_| "Invalid CRISPAWN USB data layout in the ISO.")?;
    if layout
        != json!({
            "kind": "crispawn.media_layout",
            "schema_version": 1,
            "data_volume_label": "CRISPAWN",
            "data_directory": "crispawn"
        })
    {
        return Err("The ISO does not use the CRISPAWN USB data layout.".into());
    }
    Ok(())
}

fn extract_entry(iso: &Path, member: &str, limit: u64) -> Result<Vec<u8>, String> {
    if !cfg!(windows) {
        return Err("ISO EFI extraction requires Windows.".into());
    }
    let mut child = Command::new(r"C:\Windows\System32\tar.exe")
        .arg("-xOf")
        .arg(iso)
        .arg(member)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut data = Vec::new();
    let read = child
        .stdout
        .take()
        .ok_or("Could not read ISO entry.")?
        .take(limit + 1)
        .read_to_end(&mut data);
    if read.is_err() || data.len() as u64 > limit {
        let _ = child.kill();
        let _ = child.wait();
        return Err("ISO entry exceeds the allowed size or could not be read.".into());
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    if !status.success() || data.is_empty() {
        return Err(format!("ISO entry is missing or invalid: {member}"));
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::{extract_for_iso, verify_data_layout};

    #[test]
    fn usb_layout_requires_crispawn_names() {
        assert!(verify_data_layout(br#"{"kind":"crispawn.media_layout","schema_version":1,"data_volume_label":"CRISPAWN","data_directory":"crispawn"}"#).is_ok());
        assert!(verify_data_layout(br#"{"kind":"crispawn.media_layout","schema_version":1,"data_volume_label":"PERFECTBACKUP","data_directory":"perfect-backup"}"#).is_err());
    }

    #[test]
    fn unknown_iso_is_rejected_without_creating_efi_files() {
        let temporary = tempfile::tempdir().unwrap();
        let iso = temporary.path().join("unknown.iso");
        std::fs::write(&iso, b"not a CRISPAWN ISO").unwrap();
        let output = temporary.path().join("efi");
        assert!(extract_for_iso(&iso, &output, |_| {}).is_err());
        assert!(!output.exists());
    }
}
