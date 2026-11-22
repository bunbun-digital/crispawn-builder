// Copyright (c) 2026 bunbun-digital and contributors. SPDX-License-Identifier: Apache-2.0
use std::path::Path;

use crate::Language;

pub fn available() -> bool {
    cfg!(windows) && Path::new(r"C:\Program Files\CRISPAWN-SecureBootTest-v2\request.bin").is_file()
}

/// Submits ISO bytes at normal privilege. The installed task owns VM creation.
pub fn test_iso(iso: &Path, expected: &str, language: Language) -> Result<(), String> {
    if expected.len() != 64 || !expected.bytes().all(|value| value.is_ascii_hexdigit()) {
        return Err("expected a 64-character ISO SHA-256".into());
    }
    if !available() {
        return Err("CRISPAWN-SecureBootTest-v2 is unavailable on this host".into());
    }
    let dir = tempfile::tempdir().map_err(|err| err.to_string())?;
    let sender = dir.path().join("Submit-SecureBootImage.ps1");
    std::fs::write(
        &sender,
        include_bytes!("../../../tests/Submit-SecureBootImage.ps1"),
    )
    .map_err(|err| err.to_string())?;
    let mut command =
        std::process::Command::new(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let result = command
        .args(["-NoProfile", "-NonInteractive", "-File"])
        .arg(sender)
        .arg("-Iso")
        .arg(std::path::absolute(iso).map_err(|err| err.to_string())?)
        .args(["-Sha256", expected, "-Language"])
        .arg(match language {
            Language::Japanese => "ja",
            Language::English => "en",
        })
        .output()
        .map_err(|err| err.to_string())?;
    if !result.status.success() {
        return Err(format!(
            "Hyper-V test failed: {}",
            String::from_utf8_lossy(&result.stderr).trim()
        ));
    }
    Ok(())
}
