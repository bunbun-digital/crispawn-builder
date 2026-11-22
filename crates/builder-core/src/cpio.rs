// Copyright (c) 2026 bunbun-digital and contributors. SPDX-License-Identifier: Apache-2.0
//! Bounded XZ/newc initramfs handling for LiveOS settings.

use std::collections::HashSet;
use std::io::{Cursor, Read, Write};

use xz2::read::XzDecoder;
use xz2::stream::{Check, Stream};
use xz2::write::XzEncoder;

const MAX_ROOTFS: usize = 512 * 1024 * 1024;
const MAX_ENTRIES: usize = 100_000;
const REGULAR: u32 = 0o100000;
const DIRECTORY: u32 = 0o040000;
pub(crate) const CONFIG: &str = "etc/perfect-backup/live.json";
pub(crate) const CAPABILITIES: &str = "etc/perfect-backup/release-capabilities.json";
const MARKER: &str = "etc/perfect-backup/live-root";
const CREDENTIALS: &str = "etc/crispawn/credentials.json";
const LEGACY_WIFI: &str = "etc/bunbun/wifi.conf";

pub(crate) struct Entry {
    pub(crate) name: String,
    fields: [u32; 13],
    pub(crate) data: Vec<u8>,
}

impl Entry {
    pub(crate) fn is_regular(&self) -> bool {
        self.fields[1] & 0o170000 == REGULAR
    }
    fn is_directory(&self) -> bool {
        self.fields[1] & 0o170000 == DIRECTORY
    }
    pub(crate) fn single_link(&self) -> bool {
        self.fields[4] == 1
    }
}

pub(crate) struct Archive {
    entries: Vec<Entry>,
}

impl Archive {
    pub(crate) fn read_xz(compressed: &[u8]) -> Result<Self, String> {
        let stream = Stream::new_stream_decoder(256 * 1024 * 1024, 0)
            .map_err(|error| format!("Could not initialize XZ decoder: {error}"))?;
        let mut decoder = XzDecoder::new_stream(Cursor::new(compressed), stream);
        let mut raw = Vec::new();
        (&mut decoder)
            .take(MAX_ROOTFS as u64 + 1)
            .read_to_end(&mut raw)
            .map_err(|error| format!("Could not decode initramfs: {error}"))?;
        if raw.len() > MAX_ROOTFS || decoder.total_in() as usize != compressed.len() {
            return Err("Initramfs is too large or has trailing data".into());
        }
        Self::unpack(&raw)
    }

    pub(crate) fn get(&self, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.name == name)
    }

    pub(crate) fn replace_settings(&mut self, config: &[u8]) -> Result<(), String> {
        if !(1..=65_536).contains(&config.len()) {
            return Err("LiveOS settings exceed the supported size".into());
        }
        let marker = self.get(MARKER).ok_or("Not a CRISPAWN LiveOS initramfs")?;
        if !marker.is_regular() || marker.data != b"perfect-backup-liveos-v1\n" {
            return Err("LiveOS marker is invalid".into());
        }
        for name in ["etc", "etc/perfect-backup"] {
            if !self.get(name).is_some_and(Entry::is_directory) {
                return Err("LiveOS settings directory is invalid".into());
            }
        }
        for name in [CONFIG, CREDENTIALS, LEGACY_WIFI] {
            if let Some(entry) = self.get(name) {
                if !entry.is_regular() || !entry.single_link() {
                    return Err("Unsafe configuration file in base ISO".into());
                }
            }
        }
        let inode = self
            .entries
            .iter()
            .map(|entry| entry.fields[0])
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or("CPIO inode overflow")?;
        self.entries
            .retain(|entry| ![CONFIG, CREDENTIALS, LEGACY_WIFI].contains(&entry.name.as_str()));
        let mut fields = [0u32; 13];
        fields[0] = inode;
        fields[1] = REGULAR | 0o600;
        fields[4] = 1;
        self.entries.push(Entry {
            name: CONFIG.into(),
            fields,
            data: config.to_vec(),
        });
        Ok(())
    }

    pub(crate) fn to_xz(&self) -> Result<Vec<u8>, String> {
        let raw = self.pack()?;
        let stream = Stream::new_easy_encoder(6, Check::Crc32)
            .map_err(|error| format!("Could not initialize XZ encoder: {error}"))?;
        let mut encoder = XzEncoder::new_stream(Vec::new(), stream);
        encoder
            .write_all(&raw)
            .map_err(|error| format!("Could not compress initramfs: {error}"))?;
        encoder
            .finish()
            .map_err(|error| format!("Could not finish initramfs: {error}"))
    }

    fn unpack(raw: &[u8]) -> Result<Self, String> {
        let mut entries = Vec::new();
        let mut names = HashSet::new();
        let mut position = 0usize;
        loop {
            let header = raw
                .get(position..position + 110)
                .ok_or("Truncated newc header")?;
            if &header[..6] != b"070701" {
                return Err("Unsupported initramfs archive".into());
            }
            let mut fields = [0u32; 13];
            for (index, field) in fields.iter_mut().enumerate() {
                *field = parse_hex(&header[6 + index * 8..14 + index * 8])?;
            }
            let size = fields[6] as usize;
            let namesize = fields[11] as usize;
            position += 110;
            if !(1..=4096).contains(&namesize) {
                return Err("Invalid CPIO file name size".into());
            }
            let name_bytes = raw
                .get(position..position + namesize)
                .ok_or("Truncated CPIO name")?;
            if name_bytes[namesize - 1] != 0 || name_bytes[..namesize - 1].contains(&0) {
                return Err("Invalid CPIO name terminator".into());
            }
            let original = std::str::from_utf8(&name_bytes[..namesize - 1])
                .map_err(|_| "Invalid CPIO name encoding")?;
            let name = original.strip_prefix("./").unwrap_or(original).to_owned();
            position = align4(position + namesize)?;
            if name == "TRAILER!!!" {
                if size != 0 {
                    return Err("Invalid CPIO trailer".into());
                }
                if raw
                    .get(position..)
                    .is_none_or(|tail| tail.iter().any(|&byte| byte != 0))
                {
                    return Err("Multiple or malformed CPIO archives".into());
                }
                return Ok(Self { entries });
            }
            if name.is_empty()
                || name.starts_with('/')
                || name.split('/').any(|part| part.is_empty() || part == "..")
                || !names.insert(name.clone())
            {
                return Err("Unsafe or duplicate CPIO path".into());
            }
            let data = raw
                .get(position..position.checked_add(size).ok_or("CPIO size overflow")?)
                .ok_or("Truncated CPIO file")?
                .to_vec();
            position = align4(position + size)?;
            entries.push(Entry { name, fields, data });
            if entries.len() > MAX_ENTRIES {
                return Err("Too many initramfs entries".into());
            }
        }
    }

    fn pack(&self) -> Result<Vec<u8>, String> {
        let mut output = Vec::new();
        for entry in self.entries.iter().chain(std::iter::once(&Entry {
            name: "TRAILER!!!".into(),
            fields: [0; 13],
            data: Vec::new(),
        })) {
            let mut fields = entry.fields;
            fields[6] = u32::try_from(entry.data.len()).map_err(|_| "CPIO entry too large")?;
            fields[11] = u32::try_from(entry.name.len() + 1).map_err(|_| "CPIO name too long")?;
            fields[12] = 0;
            output.extend_from_slice(b"070701");
            for value in fields {
                output.extend_from_slice(format!("{value:08x}").as_bytes());
            }
            output.extend_from_slice(entry.name.as_bytes());
            output.push(0);
            pad4(&mut output);
            output.extend_from_slice(&entry.data);
            pad4(&mut output);
            if output.len() > MAX_ROOTFS {
                return Err("Configured initramfs exceeds size limit".into());
            }
        }
        output.resize((output.len() + 511) & !511, 0);
        Ok(output)
    }
}

fn parse_hex(input: &[u8]) -> Result<u32, String> {
    let text = std::str::from_utf8(input).map_err(|_| "Invalid CPIO number")?;
    u32::from_str_radix(text, 16).map_err(|_| "Invalid CPIO number".into())
}
fn align4(value: usize) -> Result<usize, String> {
    value
        .checked_add(3)
        .map(|v| v & !3)
        .ok_or_else(|| "CPIO offset overflow".into())
}
fn pad4(output: &mut Vec<u8>) {
    output.resize((output.len() + 3) & !3, 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpio_roundtrip_and_replacement_remove_credentials() {
        let directory = |name: &str, inode| Entry {
            name: name.into(),
            fields: {
                let mut fields = [0; 13];
                fields[0] = inode;
                fields[1] = DIRECTORY | 0o755;
                fields[4] = 1;
                fields
            },
            data: Vec::new(),
        };
        let file = |name: &str, inode, data: &[u8]| Entry {
            name: name.into(),
            fields: {
                let mut fields = [0; 13];
                fields[0] = inode;
                fields[1] = REGULAR | 0o644;
                fields[4] = 1;
                fields
            },
            data: data.to_vec(),
        };
        let mut archive = Archive {
            entries: vec![
                directory("etc", 1),
                directory("etc/perfect-backup", 2),
                file(MARKER, 3, b"perfect-backup-liveos-v1\n"),
                file(CREDENTIALS, 4, b"secret"),
                file(LEGACY_WIFI, 5, b"psk=secret"),
                file(CONFIG, 6, b"{}"),
            ],
        };
        archive.replace_settings(b"{\"schema_version\":1}").unwrap();
        let xz = archive.to_xz().unwrap();
        let decoded = Archive::read_xz(&xz).unwrap();
        assert!(decoded.get(CREDENTIALS).is_none());
        assert!(decoded.get(LEGACY_WIFI).is_none());
        assert_eq!(decoded.get(CONFIG).unwrap().data, b"{\"schema_version\":1}");
        assert_eq!(decoded.get(CONFIG).unwrap().fields[1], REGULAR | 0o600);
    }
}
