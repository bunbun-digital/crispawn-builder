// Copyright (c) 2026 bunbun-digital and contributors. SPDX-License-Identifier: Apache-2.0
//! Self-contained ISO configuration. No Python, xorriso, or installed helper is needed.

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::cpio::{Archive, CAPABILITIES, CONFIG};
use crate::iso9660::Image;
use crate::runtime::image_settings;
use crate::{Project, plan, validate_project};

const ISO_LIMIT: u64 = 100_000_000;

pub struct Writer;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseSupport {
    pub tag: String,
    pub config_versions: Vec<u64>,
    pub capabilities: Vec<String>,
}

pub fn release_support(source: &Path, tag: String) -> Result<ReleaseSupport, String> {
    let source = fs::read(source).map_err(io_error)?;
    if !(1..ISO_LIMIT).contains(&(source.len() as u64)) {
        return Err("Release ISO size is invalid".into());
    }
    let image = Image::open(source)?;
    let archive = Archive::read_xz(image.read_file("boot/initrd")?)?;
    let (versions, capabilities) = release_capabilities(&archive)?;
    let mut config_versions: Vec<_> = versions.into_iter().collect();
    config_versions.sort_unstable();
    let mut capabilities: Vec<_> = capabilities.unwrap_or_default().into_iter().collect();
    capabilities.sort();
    Ok(ReleaseSupport {
        tag,
        config_versions,
        capabilities,
    })
}

impl Writer {
    /// The ISO writer is built into the executable.
    pub fn new() -> Self {
        Self
    }

    pub fn base_digest(source: &Path) -> Result<String, String> {
        let file = File::open(source).map_err(io_error)?;
        let metadata = file.metadata().map_err(io_error)?;
        if !metadata.is_file() || !(1..ISO_LIMIT).contains(&metadata.len()) {
            return Err("base ISO must be a regular file smaller than 100 MB".into());
        }
        file_digest(file)
    }

    /// Creates a new ISO from a base pinned by a trusted, out-of-band digest.
    pub fn configure(
        &self,
        project: &Project,
        source: &Path,
        output: &Path,
        expected_sha256: &str,
    ) -> Result<(), String> {
        validate_project(project)?;
        let legacy_config = if project.requires_v2() {
            None
        } else {
            Some(plan(project)?.into_bytes())
        };
        let payload = image_settings(project)?;
        if expected_sha256.len() != 64
            || !expected_sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("expected the base ISO SHA-256 as 64 hexadecimal characters".into());
        }
        let source = source.canonicalize().map_err(io_error)?;
        let output = std::path::absolute(output).map_err(io_error)?;
        if !output
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("iso"))
        {
            return Err("output must have an .iso extension".into());
        }
        let parent = output.parent().ok_or("Output directory is missing")?;
        if !parent.is_dir() {
            return Err("Output directory does not exist".into());
        }
        let sha_path = sidecar(&output, ".sha256");
        let json_path = sidecar(&output, ".json");
        for path in [&output, &sha_path, &json_path] {
            if fs::symlink_metadata(path).is_ok() {
                return Err("Output or sidecar already exists".into());
            }
        }

        let metadata = fs::metadata(&source).map_err(io_error)?;
        if !metadata.is_file() || !(1..ISO_LIMIT).contains(&metadata.len()) {
            return Err("base ISO must be a regular file smaller than 100 MB".into());
        }
        let source_bytes = fs::read(&source).map_err(io_error)?;
        if source_bytes.len() as u64 != metadata.len() {
            return Err("Input ISO changed while reading".into());
        }
        let parent_sha256 = hex::encode(Sha256::digest(&source_bytes));
        if !parent_sha256.eq_ignore_ascii_case(expected_sha256) {
            return Err("base ISO SHA-256 mismatch".into());
        }

        let mut image = Image::open(source_bytes)?;
        let boot_entries = image.boot_platforms()?;
        let signed_files = signed_file_digests(&image)?;
        let mut archive = Archive::read_xz(image.read_file("boot/initrd")?)?;
        let (settings, configuration_schema_version) =
            select_settings(&archive, legacy_config.as_deref(), &payload)?;
        archive.replace_settings(&settings)?;
        let initrd = archive.to_xz()?;
        image.replace_file("boot/initrd", &initrd, ISO_LIMIT as usize)?;

        let readback = Archive::read_xz(image.read_file("boot/initrd")?)?;
        let installed = readback
            .get(CONFIG)
            .ok_or("Configured ISO is missing live.json")?;
        if !installed.is_regular() || !installed.single_link() || installed.data != settings {
            return Err("Configured live.json readback mismatch".into());
        }
        if readback.get("etc/crispawn/credentials.json").is_some()
            || readback.get("etc/bunbun/wifi.conf").is_some()
        {
            return Err("Network credentials remain in configured ISO".into());
        }
        if image.boot_platforms()? != boot_entries || signed_file_digests(&image)? != signed_files {
            return Err("Signed boot files or boot entries changed".into());
        }
        let image_bytes = image.into_bytes();
        let iso_sha256 = hex::encode(Sha256::digest(&image_bytes));
        let configuration_sha256 = hex::encode(Sha256::digest(&settings));
        let details = json!({
            "schema_version": 1,
            "kind": "perfect-backup-configuration",
            "created_at": chrono::Utc::now().to_rfc3339(),
            "parent_iso_sha256": parent_sha256,
            "iso_sha256": iso_sha256,
            "iso_size_bytes": image_bytes.len(),
            "configuration_sha256": configuration_sha256,
            "configuration_path": format!("/{CONFIG}"),
            "configuration_schema_version": configuration_schema_version,
            "boot_entries": boot_entries,
            "kernel_preserved": true,
            "boot_tested": false,
            "contains_network_credentials": false,
        });
        publish(
            &output,
            &sha_path,
            &json_path,
            &image_bytes,
            &iso_sha256,
            &details,
        )
    }
}

fn signed_file_digests(image: &Image) -> Result<Vec<(&'static str, String)>, String> {
    let mut files = Vec::new();
    for path in [
        "boot/bzImage",
        "boot/fat.efi",
        "EFI/BOOT/BOOTX64.EFI",
        "EFI/BOOT/grubx64.efi",
        "EFI/BOOT/mmx64.efi",
    ] {
        match image.read_file(path) {
            Ok(bytes) => files.push((path, hex::encode(Sha256::digest(bytes)))),
            Err(error) if path != "boot/bzImage" && error.starts_with("ISO entry is missing:") => {}
            Err(error) => return Err(error),
        }
    }
    Ok(files)
}

fn select_settings(
    archive: &Archive,
    legacy: Option<&[u8]>,
    payload: &Value,
) -> Result<(Vec<u8>, u64), String> {
    let runtime = payload
        .get("runtime")
        .ok_or("Runtime settings are missing")?;
    if runtime["kind"] != "crispawn.live"
        || runtime["schema_version"] != 2
        || runtime["network"] != json!({"mode": "none"})
        || !payload["credentials"].is_null()
    {
        return Err(
            "Community Builder requires a USB-only runtime without network credentials".into(),
        );
    }
    let (versions, capabilities) = release_capabilities(archive)?;
    if let Some(capabilities) = capabilities {
        let required = required_capabilities(runtime)?;
        if !required.is_subset(&capabilities) {
            if required.contains("processing.verify_after_write")
                && !capabilities.contains("processing.verify_after_write")
            {
                return Err("The selected ISO cannot skip backup readback. Use a CRISPAWN release that declares processing.verify_after_write".into());
            }
            return Err(
                "The selected base ISO does not declare support for the requested LiveOS settings"
                    .into(),
            );
        }
    }
    if !versions.contains(&2) {
        if !versions.contains(&1) {
            return Err("Unsupported LiveOS configuration version".into());
        }
        if let Some(existing) = archive.get(CONFIG) {
            let old: Value =
                serde_json::from_slice(&existing.data).map_err(|_| "Invalid base ISO runtime")?;
            let version = old
                .get("schema_version")
                .map(Value::as_u64)
                .unwrap_or(Some(1));
            if old["kind"] == "crispawn.live"
                && version == Some(2)
                && archive.get(CAPABILITIES).is_none()
            {
                return Err(
                    "Base ISO uses runtime v2 but has no release capability metadata".into(),
                );
            }
            if !old.is_object() || old["kind"] == "crispawn.live" || version != Some(1) {
                return Err("Base runtime does not match its release declaration".into());
            }
        }
        return legacy.map(|bytes| (bytes.to_vec(), 1)).ok_or_else(|| {
            "The selected base ISO cannot apply these LiveOS settings; runtime v2 is required"
                .into()
        });
    }
    let mut settings = serde_json::to_vec_pretty(runtime).map_err(|error| error.to_string())?;
    settings.push(b'\n');
    if settings.len() > 65_536 {
        return Err("Runtime settings exceed the size limit".into());
    }
    Ok((settings, 2))
}

fn release_capabilities(
    archive: &Archive,
) -> Result<(HashSet<u64>, Option<HashSet<String>>), String> {
    let Some(entry) = archive.get(CAPABILITIES) else {
        return Ok((HashSet::from([1]), None));
    };
    if !entry.is_regular() || !entry.single_link() || !(1..=65_536).contains(&entry.data.len()) {
        return Err("Invalid release capability file".into());
    }
    let document: Value =
        serde_json::from_slice(&entry.data).map_err(|_| "Invalid release capability JSON")?;
    let object = document
        .as_object()
        .ok_or("Invalid release capability document")?;
    let keys = [
        "kind",
        "schema_version",
        "release_id",
        "architecture",
        "kernel_build",
        "live_config_versions",
        "capabilities",
    ];
    if object.len() != keys.len()
        || keys.iter().any(|key| !object.contains_key(*key))
        || document["kind"] != "crispawn.release_capabilities"
        || document["schema_version"] != 1
        || document["architecture"] != "x86_64"
    {
        return Err("Invalid release capability document".into());
    }
    for key in ["release_id", "kernel_build"] {
        let text = document[key].as_str().ok_or("Invalid release identity")?;
        if text.is_empty()
            || text.len() > 128
            || !text.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-')
            })
        {
            return Err("Invalid release identity".into());
        }
    }
    let listed_versions = document["live_config_versions"]
        .as_array()
        .ok_or("Invalid release versions")?;
    let listed_caps = document["capabilities"]
        .as_array()
        .ok_or("Invalid release capabilities")?;
    if !(1..=16).contains(&listed_versions.len()) || !(1..=128).contains(&listed_caps.len()) {
        return Err("Invalid release version or capability count".into());
    }
    let mut versions = HashSet::new();
    for item in listed_versions {
        let version = item
            .as_u64()
            .filter(|value| (1..=65_535).contains(value))
            .ok_or("Invalid release version")?;
        if !versions.insert(version) {
            return Err("Duplicate release version".into());
        }
    }
    let mut capabilities = HashSet::new();
    for item in listed_caps {
        let value = item.as_str().ok_or("Invalid release capability")?;
        if !(2..=96).contains(&value.len())
            || !value.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
            || !value.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'.' | b'_' | b'-')
            })
            || !capabilities.insert(value.to_owned())
        {
            return Err("Invalid release capability".into());
        }
    }
    Ok((versions, Some(capabilities)))
}

fn required_capabilities(runtime: &Value) -> Result<HashSet<String>, String> {
    let paths: &[(&str, &str)] = &[
        ("sources", "mode"),
        ("startup", "mode"),
        ("destination", "kind"),
        ("completion", "action"),
        ("ui", "mode"),
        ("ui", "language"),
        ("logging", "policy"),
        ("processing", "initial_profile"),
    ];
    let prefixes = [
        "sources.",
        "startup.",
        "destination.",
        "completion.",
        "ui.",
        "ui.lang.",
        "logging.",
        "processing.profile.",
    ];
    let mut result = HashSet::from(["processing.auto".to_owned()]);
    for ((parent, field), prefix) in paths.iter().zip(prefixes) {
        let value = runtime[parent][field]
            .as_str()
            .ok_or("Invalid runtime capability field")?;
        result.insert(format!("{prefix}{value}"));
    }
    if runtime["failure"]["action"] == "stay" {
        result.insert("failure.stay".to_owned());
    }
    if runtime["processing"]["verify_after_write"] == false {
        result.insert("processing.verify_after_write".to_owned());
    }
    Ok(result)
}

fn sidecar(output: &Path, suffix: &str) -> PathBuf {
    let mut name: OsString = output.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn publish(
    output: &Path,
    sha_path: &Path,
    json_path: &Path,
    bytes: &[u8],
    sha: &str,
    details: &Value,
) -> Result<(), String> {
    let mut staged =
        tempfile::NamedTempFile::new_in(output.parent().ok_or("Output directory is missing")?)
            .map_err(io_error)?;
    staged
        .write_all(bytes)
        .and_then(|_| staged.as_file().sync_all())
        .map_err(io_error)?;
    let checksum = format!(
        "{sha}  {}\n",
        output
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("image.iso")
    );
    let mut metadata = serde_json::to_vec_pretty(details).map_err(|error| error.to_string())?;
    metadata.push(b'\n');
    let mut staged_sha = stage_file(sha_path, checksum.as_bytes())?;
    let mut staged_json = stage_file(json_path, &metadata)?;
    let mut sha_owned = false;
    let mut json_owned = false;
    let result = (|| -> Result<(), String> {
        staged_sha.as_file_mut().sync_all().map_err(io_error)?;
        staged_json.as_file_mut().sync_all().map_err(io_error)?;
        staged_sha
            .persist_noclobber(sha_path)
            .map_err(|error| error.error.to_string())?;
        sha_owned = true;
        staged_json
            .persist_noclobber(json_path)
            .map_err(|error| error.error.to_string())?;
        json_owned = true;
        staged
            .persist_noclobber(output)
            .map_err(|error| error.error.to_string())?;
        Ok(())
    })();
    if result.is_err() {
        if sha_owned {
            let _ = fs::remove_file(sha_path);
        }
        if json_owned {
            let _ = fs::remove_file(json_path);
        }
    }
    result
}

fn stage_file(path: &Path, bytes: &[u8]) -> Result<tempfile::NamedTempFile, String> {
    let mut file =
        tempfile::NamedTempFile::new_in(path.parent().ok_or("Output directory is missing")?)
            .map_err(io_error)?;
    file.write_all(bytes).map_err(io_error)?;
    Ok(file)
}

fn file_digest(file: File) -> Result<String, String> {
    let mut input = file.take(ISO_LIMIT);
    let mut digest = Sha256::new();
    let mut buffer = [0; 32 * 1024];
    let mut size = 0;
    loop {
        let read = input.read(&mut buffer).map_err(io_error)?;
        if read == 0 {
            break;
        }
        size += read as u64;
        digest.update(&buffer[..read]);
    }
    if !(1..ISO_LIMIT).contains(&size) {
        return Err("base ISO must be nonempty and smaller than 100 MB".into());
    }
    Ok(hex::encode(digest.finalize()))
}

fn io_error(error: io::Error) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optional_runtime_features_require_release_capabilities() {
        let mut project = Project::default();
        project.options.verify_after_write = false;
        project.options.failure_action = "stay".into();
        let runtime = crate::runtime::image_settings(&project).unwrap();
        let required = required_capabilities(&runtime["runtime"]).unwrap();
        assert!(required.contains("processing.verify_after_write"));
        assert!(required.contains("failure.stay"));
    }
}
