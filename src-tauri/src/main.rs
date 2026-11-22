// Copyright (c) 2026 bunbun-digital and contributors. SPDX-License-Identifier: Apache-2.0
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use crispawn_builder_core::efi;
use crispawn_builder_core::image::Writer;
use crispawn_builder_core::usb::{self, InternalDisk, PreparedUsbDisk};
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
async fn prepared_usb_disks() -> Result<Vec<PreparedUsbDisk>, String> {
    tauri::async_runtime::spawn_blocking(usb::list_prepared)
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
    disk: PreparedUsbDisk,
    settings: Settings,
    pc_mode: String,
    selected_disk_numbers: Vec<u32>,
    ui_language: String,
) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if !settings.source_ids.is_empty() || settings.language != SettingsLanguage::Ja
            || settings.start_seconds != 30 || settings.poweroff_seconds != Some(5) {
            return Err("The selected ISO has different startup or source settings. Select a matching ISO release.".into());
        }
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
        let Some(iso) = app.dialog().file().add_filter("Debian LiveOS ISO", &["iso"]).blocking_pick_file() else {
            return Ok(false);
        };
        let iso = iso.into_path().map_err(|error| error.to_string())?;
        let staging = tempfile::tempdir().map_err(|error| error.to_string())?;
        usb_progress(&app, "checkingIso", 1);
        let size = std::fs::metadata(&iso).map_err(|error| error.to_string())?.len();
        if size == 0 || size >= 100_000_000 {
            return Err("The selected ISO must be smaller than 100 MB.".into());
        }
        let staged_iso = staging.path().join("base.iso");
        std::fs::copy(&iso, &staged_iso).map_err(|error| error.to_string())?;
        let efi_dir = staging.path().join("efi");
        let expected = efi::extract_for_iso(&staged_iso, &efi_dir, |percent| {
            usb_progress(&app, "extractingEfi", percent);
        })?;
        usb_progress(&app, "awaitingConfirmation", 16);
        let text = if ui_language == "ja" {
            format!("{}（{} GB、ディスク {}）にISOを書き込みます。\n\nESPの起動ファイルを更新します。NTFSのバックアップ領域は維持します。続行しますか？",
                    disk.friendly_name, disk.size_bytes / 1_000_000_000, disk.disk_number)
        } else {
            format!("Install the ISO on {} ({} GB, disk {})?\n\nBoot files on the ESP will be updated. The NTFS backup area is preserved.",
                    disk.friendly_name, disk.size_bytes / 1_000_000_000, disk.disk_number)
        };
        if !app.dialog().message(text).title("CRISPAWN Builder")
            .buttons(tauri_plugin_dialog::MessageDialogButtons::OkCancel).blocking_show() {
            return Ok(false);
        }
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?.as_nanos();
        let report = std::env::temp_dir().join(format!("crispawn-usb-{nonce}"));
        usb::install_prepared_with_progress(&staged_iso, &efi_dir, &expected, disk.disk_number,
            &disk.unique_id, disk.size_bytes, &report, false, |progress| {
                let _ = app.emit("usb-write-progress", progress);
            })?;
        Ok(true)
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
async fn save_iso(
    app: tauri::AppHandle,
    settings: Settings,
    base_sha256: String,
) -> Result<bool, String> {
    let writer = Writer::new();
    let project = settings.project();
    plan(&project)?;
    tauri::async_runtime::spawn_blocking(move || {
        let Some(source) = app
            .dialog()
            .file()
            .add_filter("Base ISO", &["iso"])
            .blocking_pick_file()
        else {
            return Ok(false);
        };
        let Some(output) = app
            .dialog()
            .file()
            .add_filter("Configured ISO", &["iso"])
            .set_file_name("crispawn.iso")
            .blocking_save_file()
        else {
            return Ok(false);
        };
        let source = source.into_path().map_err(|error| error.to_string())?;
        let output = output.into_path().map_err(|error| error.to_string())?;
        writer.configure(&project, &source, &output, &base_sha256)?;
        Ok(true)
    })
    .await
    .map_err(|error| error.to_string())?
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            plan_settings,
            prepared_usb_disks,
            internal_disks,
            save_usb,
            save_iso
        ])
        .run(tauri::generate_context!())
        .expect("could not launch CRISPAWN Builder");
}
