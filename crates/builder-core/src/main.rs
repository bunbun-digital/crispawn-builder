// Copyright (c) 2026 bunbun-digital and contributors. SPDX-License-Identifier: Apache-2.0

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use crispawn_builder_core::image::Writer;
use crispawn_builder_core::{Completion, Language, Project, plan};

const HELP: &str = "Usage: crispawn-builder plan [settings]\n\
    crispawn-builder iso BASE.iso OUTPUT.iso BASE_SHA256 [settings]\n\
    crispawn-builder usb-list\n\
    crispawn-builder disk-list\n\
    crispawn-builder efi-extract BASE.iso OUTPUT_DIRECTORY\n\
    crispawn-builder usb BASE.iso EFI_DIRECTORY ISO_SHA256 DISK_NUMBER DISK_UNIQUE_ID DISK_BYTES REPORT_DIR [--preflight]\n\
    Settings: [--language ja|en] [--source-id LINUX_ID]... [--start-seconds N] [--poweroff-seconds N | --stay]";

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let iso = match args.next().as_deref() {
        Some("--help" | "-h") => {
            println!("{HELP}");
            return Ok(());
        }
        Some("plan") => None,
        Some("usb-list") => {
            if args.next().is_some() {
                return Err(HELP.into());
            }
            println!(
                "{}",
                serde_json::to_string_pretty(&crispawn_builder_core::usb::list_prepared()?)
                    .map_err(|error| error.to_string())?
            );
            return Ok(());
        }
        Some("disk-list") => {
            if args.next().is_some() {
                return Err(HELP.into());
            }
            println!(
                "{}",
                serde_json::to_string_pretty(&crispawn_builder_core::usb::list_internal()?)
                    .map_err(|error| error.to_string())?
            );
            return Ok(());
        }
        Some("efi-extract") => {
            let iso = PathBuf::from(args.next().ok_or(HELP)?);
            let output = PathBuf::from(args.next().ok_or(HELP)?);
            if args.next().is_some() {
                return Err(HELP.into());
            }
            crispawn_builder_core::efi::extract_for_iso(&iso, &output, |_| {})?;
            println!("Signed EFI files extracted and verified from ISO.");
            return Ok(());
        }
        Some("usb") => {
            let iso = PathBuf::from(args.next().ok_or(HELP)?);
            let efi_dir = PathBuf::from(args.next().ok_or(HELP)?);
            let digest = args.next().ok_or(HELP)?;
            let disk_number = args.next().ok_or(HELP)?.parse::<u32>().map_err(|_| HELP)?;
            let unique_id = args.next().ok_or(HELP)?;
            let disk_bytes = args.next().ok_or(HELP)?.parse::<u64>().map_err(|_| HELP)?;
            let report_dir = PathBuf::from(args.next().ok_or(HELP)?);
            let preflight = match args.next().as_deref() {
                None => false,
                Some("--preflight") => true,
                _ => return Err(HELP.into()),
            };
            if args.next().is_some() {
                return Err(HELP.into());
            }
            crispawn_builder_core::usb::install_prepared(
                &iso,
                &efi_dir,
                &digest,
                disk_number,
                &unique_id,
                disk_bytes,
                &report_dir,
                preflight,
            )?;
            return Ok(());
        }
        Some("iso") => Some((
            PathBuf::from(args.next().ok_or(HELP)?),
            PathBuf::from(args.next().ok_or(HELP)?),
            args.next().ok_or(HELP)?,
        )),
        _ => return Err(HELP.into()),
    };
    let mut project = Project::default();
    let mut completion_selected = false;
    while let Some(flag) = args.next() {
        let mut value = || {
            args.next()
                .ok_or_else(|| format!("{flag} requires a value"))
        };
        match flag.as_str() {
            "--language" => {
                project.language = match value()?.as_str() {
                    "ja" => Language::Japanese,
                    "en" => Language::English,
                    _ => return Err("language must be ja or en".into()),
                };
            }
            "--source-id" => project.source_ids.push(value()?),
            "--start-seconds" => project.start_seconds = seconds(&value()?, &flag)?,
            "--stay" | "--poweroff-seconds" => {
                if completion_selected {
                    return Err("choose one completion action".into());
                }
                completion_selected = true;
                project.completion = if flag == "--stay" {
                    Completion::Stay
                } else {
                    Completion::PowerOff {
                        seconds: seconds(&value()?, &flag)?,
                    }
                };
            }
            _ => return Err(format!("unsupported option: {flag}")),
        }
    }
    if let Some((source, output, digest)) = iso {
        let writer = Writer::new();
        writer.configure(&project, &source, &output, &digest)?;
        println!("ISO configured and verified: {}", output.display());
        return Ok(());
    }
    let settings = plan(&project)?;
    io::stdout()
        .lock()
        .write_all(settings.as_bytes())
        .map_err(|error| error.to_string())
}

fn seconds(value: &str, flag: &str) -> Result<u32, String> {
    value
        .parse()
        .map_err(|_| format!("{flag} requires a nonnegative integer"))
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
