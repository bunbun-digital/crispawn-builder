// Copyright (c) 2026 bunbun-digital and contributors. SPDX-License-Identifier: Apache-2.0

use std::env;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use sha2::{Digest, Sha256};

use crate::runtime::image_settings;
use crate::{Project, plan};

const ISO_LIMIT: u64 = 100_000_000;
const CONFIGURE: &str = include_str!("../../../tools/configure-backup.py");
const ISO_UTILITIES: &str = include_str!("../../../tools/iso_utils.py");

pub struct Writer {
    python: PathBuf,
    xorriso: PathBuf,
}

impl Writer {
    pub fn from_environment() -> Result<Self, String> {
        let bundled = env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.join("tools")));
        Ok(Self {
            python: executable(bundled.as_deref(), "python/python.exe", "CRISPAWN_PYTHON")?,
            xorriso: executable(
                bundled.as_deref(),
                "xorriso/usr/bin/xorriso.exe",
                "CRISPAWN_XORRISO",
            )?,
        })
    }

    pub fn base_digest(source: &Path) -> Result<String, String> {
        let file = File::open(source).map_err(io_error)?;
        let metadata = file.metadata().map_err(io_error)?;
        if !metadata.is_file() || !(1..ISO_LIMIT).contains(&metadata.len()) {
            return Err("base ISO must be a regular file smaller than 100 MB".into());
        }
        file_digest(file)
    }

    /// Builds from an explicitly selected base pinned by a trusted, out-of-band digest.
    pub fn configure(
        &self,
        project: &Project,
        source: &Path,
        output: &Path,
        expected_sha256: &str,
    ) -> Result<(), String> {
        let config = plan(project)?;
        let payload = serde_json::to_vec(&image_settings(project)?)
            .map_err(|_| "Could not serialize the runtime settings.")?;
        if expected_sha256.len() != 64 || !expected_sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("expected the base ISO SHA-256 as 64 hexadecimal characters".into());
        }
        let source = source.canonicalize().map_err(io_error)?;
        let output = std::path::absolute(output).map_err(io_error)?;
        if !output
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("iso"))
        {
            return Err("output must have an .iso extension".into());
        }
        let metadata = fs::metadata(&source).map_err(io_error)?;
        if !metadata.is_file() || !(1..ISO_LIMIT).contains(&metadata.len()) {
            return Err("base ISO must be a regular file smaller than 100 MB".into());
        }

        let work = tempfile::tempdir().map_err(io_error)?;
        let base = work.path().join("base.iso");
        let copied = io::copy(
            &mut File::open(source).map_err(io_error)?.take(ISO_LIMIT),
            &mut File::create(&base).map_err(io_error)?,
        )
        .map_err(io_error)?;
        if !(1..ISO_LIMIT).contains(&copied) {
            return Err("base ISO must be nonempty and smaller than 100 MB".into());
        }
        let actual_sha256 = file_digest(File::open(&base).map_err(io_error)?)?;
        if !actual_sha256.eq_ignore_ascii_case(expected_sha256) {
            return Err("base ISO SHA-256 mismatch".into());
        }
        let script = work.path().join("configure-backup.py");
        let settings = work.path().join("settings.json");
        fs::write(&script, CONFIGURE).map_err(io_error)?;
        fs::write(work.path().join("iso_utils.py"), ISO_UTILITIES).map_err(io_error)?;
        fs::write(&settings, config).map_err(io_error)?;
        let mut command = Command::new(&self.python);
        command
            .args(["-I", "-B"])
            .arg(script)
            .args([&base, &output])
            .arg("--config")
            .arg(settings)
            .arg("--xorriso")
            .arg(&self.xorriso);
        let mut child = command
            .arg("--runtime-v2-stdin")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(io_error)?;
        let send = child.stdin.take().expect("piped stdin").write_all(&payload);
        let status = child.wait().map_err(io_error)?;
        if status.code() == Some(3) {
            return Err(
                "The selected base ISO does not declare support for the requested LiveOS settings."
                    .into(),
            );
        }
        send.map_err(|_| "Could not pass runtime settings to the ISO writer.")?;
        if !status.success() {
            return Err(format!("ISO assembly or verification failed ({})", status));
        }
        Ok(())
    }
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

fn executable(bundled: Option<&Path>, relative: &str, name: &str) -> Result<PathBuf, String> {
    if let Some(path) = bundled
        .map(|dir| dir.join(relative))
        .filter(|path| path.is_file())
    {
        return Ok(path);
    }
    let path = env::var_os(name).map(PathBuf::from);
    path.filter(|path| path.is_absolute() && path.is_file())
        .ok_or_else(|| format!("set {name} to an absolute path to the trusted executable"))
}

fn io_error(error: io::Error) -> String {
    error.to_string()
}
