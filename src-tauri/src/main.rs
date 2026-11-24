// Copyright (c) 2026 bunbun-digital and contributors. SPDX-License-Identifier: Apache-2.0
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use crispawn_builder_core::efi;
use crispawn_builder_core::image::Writer;
use crispawn_builder_core::release;
use crispawn_builder_core::usb::{self, InternalDisk, UsbDisk};
use crispawn_builder_core::{Completion, Language, Project, plan};
use serde::Deserialize;
use tauri::Emitter;
use tauri_plugin_dialog::DialogExt;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Settings {
    source_ids: Vec<String>,
    language: SettingsLanguage,
    start_seconds: u32,
    poweroff_seconds: Option<u32>,
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum SettingsLanguage {
    Ja,
    En,
}

impl Settings {
    fn project(self) -> Project {
        Project {
            source_ids: self.source_ids,
            language: match self.language {
                SettingsLanguage::Ja => Language::Japanese,
                SettingsLanguage::En => Language::English,
            },
            start_seconds: self.start_seconds,
            completion: self
                .poweroff_seconds
                .map_or(Completion::Stay, |seconds| Completion::PowerOff { seconds }),
        }
    }
}

#[tauri::command]
fn plan_settings(settings: Settings) -> Result<String, &'static str> {
    plan(&settings.project())
}

#[tauri::command]
async fn usb_disks() -> Result<Vec<UsbDisk>, String> {
    tauri::async_runtime::spawn_blocking(usb::list_usb)
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn internal_disks() -> Result<Vec<InternalDisk>, String> {
    tauri::async_runtime::spawn_blocking(usb::list_internal)
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn save_usb(
    app: tauri::AppHandle,
    disk: UsbDisk,
    reinitialize_usb: bool,
    settings: Settings,
    pc_mode: String,
    selected_disk_numbers: Vec<u32>,
    ui_language: String,
) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let project = settings.project();
        plan(&project)?;
        match pc_mode.as_str() {
            "local" => {
                let internal = usb::list_internal()?;
                let mut all: Vec<_> = internal.iter().map(|disk| disk.disk_number).collect();
                let mut selected = selected_disk_numbers;
                all.sort_unstable();
                selected.sort_unstable();
                selected.dedup();
                if all.is_empty() || selected != all {
                    return Err("This ISO selects all internal disks. Choose all disks or use a release with verified Linux disk IDs.".into());
                }
            }
            "other" => {}
            _ => return Err("Invalid PC selection".into()),
        }
        usb_progress(&app, "checkingRelease", 0);
        let latest = release::latest_iso(|percent| usb_progress(&app, "downloadingIso", 2 + (u16::from(percent) * 8 / 100) as u8))?;
        eprintln!("Using CRISPAWN release {} ({})", latest.tag, if latest.downloaded { "downloaded" } else { "cached" });
        let staging = tempfile::tempdir().map_err(|error| error.to_string())?;
        usb_progress(&app, "checkingIso", 11);
        let size = std::fs::metadata(&latest.path).map_err(|error| error.to_string())?.len();
        if size == 0 || size >= 100_000_000 {
            return Err("The release ISO must be smaller than 100 MB.".into());
        }
        let base_iso = staging.path().join("base.iso");
        std::fs::copy(&latest.path, &base_iso).map_err(|error| error.to_string())?;
        if !Writer::base_digest(&base_iso)?.eq_ignore_ascii_case(&latest.sha256) {
            return Err("Cached ISO changed while preparing the USB".into());
        }
        usb_progress(&app, "configuringIso", 12);
        let staged_iso = staging.path().join("configured.iso");
        Writer::new().configure(&project, &base_iso, &staged_iso, &latest.sha256)?;
        let efi_dir = staging.path().join("efi");
        let expected = efi::extract_for_github_iso(&staged_iso, &efi_dir, |percent| {
            usb_progress(&app, "extractingEfi", 13 + percent / 4);
        })?;
        usb_progress(&app, "awaitingConfirmation", 16);
        let text = if ui_language == "ja" {
            if disk.prepared && !reinitialize_usb {
                format!("{}（{} GB、ディスク {}）にISOを書き込みます。\n\nESPの起動ファイルを更新します。NTFSのバックアップ領域は維持します。続行しますか？",
                    disk.friendly_name, disk.size_bytes / 1_000_000_000, disk.disk_number)
            } else {
                format!("{}（{} GB、ディスク {}）を初期化してISOを書き込みます。\n\nこのUSBディスクの全パーティションとデータを削除し、起動領域とNTFS保存領域を新しく作ります。復元できません。続行しますか？",
                    disk.friendly_name, disk.size_bytes / 1_000_000_000, disk.disk_number)
            }
        } else {
            if disk.prepared && !reinitialize_usb {
                format!("Install the ISO on {} ({} GB, disk {})?\n\nBoot files on the ESP will be updated. The NTFS backup area is preserved.",
                    disk.friendly_name, disk.size_bytes / 1_000_000_000, disk.disk_number)
            } else {
                format!("Initialize {} ({} GB, disk {}) and install the ISO?\n\nAll partitions and data on this USB disk will be erased. A new boot partition and NTFS data partition will be created. This cannot be undone.",
                    disk.friendly_name, disk.size_bytes / 1_000_000_000, disk.disk_number)
            }
        };
        if !app.dialog().message(text).title("CRISPAWN Builder")
            .buttons(tauri_plugin_dialog::MessageDialogButtons::OkCancel).blocking_show() {
            return Ok(None);
        }
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?.as_nanos();
        let report = std::env::temp_dir().join(format!("crispawn-usb-{nonce}"));
        let written_iso_name = usb::install_selected(&staged_iso, &efi_dir, &expected, &disk, reinitialize_usb, &report, |progress| {
                let _ = app.emit("usb-write-progress", progress);
            })?;
        Ok(Some(written_iso_name))
    }).await.map_err(|error| error.to_string())?
}

fn usb_progress(app: &tauri::AppHandle, phase: &str, percent: u8) {
    let _ = app.emit(
        "usb-write-progress",
        usb::UsbProgress {
            phase: phase.to_owned(),
            percent,
        },
    );
}

#[tauri::command]
async fn save_iso(app: tauri::AppHandle, settings: Settings) -> Result<bool, String> {
    let writer = Writer::new();
    let project = settings.project();
    plan(&project)?;
    tauri::async_runtime::spawn_blocking(move || {
        let Some(output) = app
            .dialog()
            .file()
            .add_filter("Configured ISO", &["iso"])
            .set_file_name("crispawn.iso")
            .blocking_save_file()
        else {
            return Ok(false);
        };
        let output = output.into_path().map_err(|error| error.to_string())?;
        usb_progress(&app, "checkingRelease", 0);
        let latest = release::latest_iso(|percent| {
            usb_progress(
                &app,
                "downloadingIso",
                2 + (u16::from(percent) * 13 / 100) as u8,
            )
        })?;
        eprintln!(
            "Using CRISPAWN release {} ({})",
            latest.tag,
            if latest.downloaded {
                "downloaded"
            } else {
                "cached"
            }
        );
        usb_progress(&app, "configuringIso", 20);
        writer.configure(&project, &latest.path, &output, &latest.sha256)?;
        usb_progress(&app, "complete", 100);
        Ok(true)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
fn exit_builder(app: tauri::AppHandle) {
    app.exit(0);
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            plan_settings,
            usb_disks,
            internal_disks,
            save_usb,
            save_iso,
            exit_builder
        ])
        .run(tauri::generate_context!())
        .expect("could not launch CRISPAWN Builder");
}
