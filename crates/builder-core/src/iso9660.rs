// Copyright (c) 2026 bunbun-digital and contributors. SPDX-License-Identifier: Apache-2.0
//! Strict ISO9660 reader and append-only file replacement for CRISPAWN images.
//! Existing boot images and signed files remain at their original extents.

use std::collections::BTreeSet;

const BLOCK: usize = 2048;
const MAX_DESCRIPTORS: usize = 64;

pub(crate) struct Image {
    bytes: Vec<u8>,
    trees: Vec<(usize, bool)>, // descriptor offset, Joliet
    boot_catalog: Option<(usize, [u8; BLOCK])>,
}

#[derive(Clone, Copy)]
struct Record {
    offset: usize,
    extent: u32,
    size: u32,
    flags: u8,
}

impl Image {
    pub(crate) fn open(bytes: Vec<u8>) -> Result<Self, String> {
        if bytes.len() < 18 * BLOCK || bytes.len() % BLOCK != 0 {
            return Err("ISO is truncated or not block aligned".into());
        }
        // The append-only writer cannot repair hybrid MBR/GPT backup tables.
        if bytes.get(510..512) == Some(&[0x55, 0xaa]) || bytes.get(512..520) == Some(b"EFI PART") {
            return Err("Hybrid partitioned ISOs are not supported for settings updates".into());
        }
        let mut trees = Vec::new();
        let mut boot_catalog = None;
        let mut terminated = false;
        for sector in 16..16 + MAX_DESCRIPTORS {
            let offset = sector * BLOCK;
            let descriptor = bytes
                .get(offset..offset + BLOCK)
                .ok_or("ISO descriptors are truncated")?;
            if &descriptor[1..6] != b"CD001" || descriptor[6] != 1 {
                return Err("Invalid ISO volume descriptor".into());
            }
            match descriptor[0] {
                0 => {
                    if descriptor[7..39].starts_with(b"EL TORITO SPECIFICATION") {
                        if boot_catalog.is_some() {
                            return Err("Multiple El Torito catalogs are unsupported".into());
                        }
                        let lba = le32(&descriptor[71..75]) as usize;
                        let start = lba
                            .checked_mul(BLOCK)
                            .ok_or("Boot catalog offset overflow")?;
                        let catalog = bytes
                            .get(start..start + BLOCK)
                            .ok_or("Boot catalog is outside ISO")?;
                        boot_catalog = Some((start, catalog.try_into().expect("2048 bytes")));
                    }
                }
                1 | 2 => {
                    let joliet = descriptor[0] == 2;
                    if joliet && !matches!(&descriptor[88..91], b"%/@" | b"%/C" | b"%/E") {
                        return Err("Unsupported supplementary ISO descriptor".into());
                    }
                    if le16(&descriptor[128..130]) != BLOCK as u16
                        || be16(&descriptor[130..132]) != BLOCK as u16
                    {
                        return Err("Unsupported ISO block size".into());
                    }
                    let blocks = both32(&descriptor[80..88])? as usize;
                    if blocks == 0 || blocks > bytes.len() / BLOCK {
                        return Err("Invalid ISO volume size".into());
                    }
                    let root = record(&bytes, offset + 156)?;
                    if root.flags & 2 == 0 {
                        return Err("ISO root is not a directory".into());
                    }
                    trees.push((offset, joliet));
                }
                255 => {
                    terminated = true;
                    break;
                }
                _ => return Err("Unsupported ISO volume descriptor".into()),
            }
        }
        if !terminated || trees.is_empty() || trees[0].0 != 16 * BLOCK || trees[0].1 {
            return Err("ISO primary volume descriptor is missing".into());
        }
        Ok(Self {
            bytes,
            trees,
            boot_catalog,
        })
    }

    pub(crate) fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    pub(crate) fn read_file(&self, path: &str) -> Result<&[u8], String> {
        let (offset, _) = self.trees[0];
        let item = self.find_file(offset, false, path)?;
        if item.flags & 2 != 0 || item.flags & 0x80 != 0 {
            return Err(format!("Unsupported ISO file: {path}"));
        }
        self.data(item)
    }

    pub(crate) fn replace_file(
        &mut self,
        path: &str,
        replacement: &[u8],
        limit: usize,
    ) -> Result<(), String> {
        if replacement.is_empty() || replacement.len() > u32::MAX as usize {
            return Err("Replacement ISO file has an invalid size".into());
        }
        let primary = self.read_file(path)?.to_vec();
        let mut positions = BTreeSet::new();
        for &(descriptor, joliet) in &self.trees {
            let item = self.find_file(descriptor, joliet, path)?;
            if item.flags & 2 != 0 || item.flags & 0x80 != 0 || self.data(item)? != primary {
                return Err("ISO directory trees disagree about the target file".into());
            }
            positions.insert(item.offset);
        }
        let padding = (BLOCK - replacement.len() % BLOCK) % BLOCK;
        let new_len = self
            .bytes
            .len()
            .checked_add(replacement.len())
            .and_then(|n| n.checked_add(padding))
            .ok_or("ISO size overflow")?;
        if new_len >= limit || new_len / BLOCK > u32::MAX as usize {
            return Err("Configured ISO exceeds the 100 MB limit".into());
        }
        let extent = u32::try_from(self.bytes.len() / BLOCK).map_err(|_| "ISO extent overflow")?;
        let size = replacement.len() as u32;
        self.bytes.extend_from_slice(replacement);
        self.bytes.resize(new_len, 0);
        for position in positions {
            write_both32(&mut self.bytes[position + 2..position + 10], extent);
            write_both32(&mut self.bytes[position + 10..position + 18], size);
        }
        for &(descriptor, _) in &self.trees {
            write_both32(
                &mut self.bytes[descriptor + 80..descriptor + 88],
                (new_len / BLOCK) as u32,
            );
        }
        if self.read_file(path)? != replacement {
            return Err("Configured ISO file readback differs from requested bytes".into());
        }
        if let Some((offset, original)) = &self.boot_catalog {
            if self.bytes[*offset..*offset + BLOCK] != original[..] {
                return Err("ISO boot catalog changed".into());
            }
        }
        Ok(())
    }

    pub(crate) fn boot_platforms(&self) -> Result<Vec<&'static str>, String> {
        let Some((_, catalog)) = &self.boot_catalog else {
            return Ok(Vec::new());
        };
        if catalog[0] != 1 || catalog[30..32] != [0x55, 0xaa] {
            return Err("Invalid El Torito boot catalog".into());
        }
        let mut platforms = BTreeSet::new();
        if catalog[32] == 0x88 {
            platforms.insert(platform(catalog[1]));
        }
        let mut offset = 64;
        while offset + 32 <= BLOCK {
            match catalog[offset] {
                0x90 | 0x91 => {
                    let count = le16(&catalog[offset + 2..offset + 4]) as usize;
                    if count == 0 || offset + 32 + count * 32 > BLOCK {
                        return Err("Invalid El Torito section".into());
                    }
                    if catalog[offset + 32] == 0x88 {
                        platforms.insert(platform(catalog[offset + 1]));
                    }
                    offset += 32 + count * 32;
                }
                0 if catalog[offset..].iter().all(|&byte| byte == 0) => break,
                _ => return Err("Unsupported El Torito section".into()),
            }
        }
        Ok(platforms.into_iter().collect())
    }

    fn find_file(&self, descriptor: usize, joliet: bool, path: &str) -> Result<Record, String> {
        let mut current = record(&self.bytes, descriptor + 156)?;
        for component in path.trim_start_matches('/').split('/') {
            let directory = self.data(current)?;
            let mut position = 0;
            let mut found = None;
            while position < directory.len() {
                let size = directory[position] as usize;
                if size == 0 {
                    position = (position / BLOCK + 1) * BLOCK;
                    continue;
                }
                if position + size > directory.len() {
                    return Err("ISO directory record exceeds directory".into());
                }
                let offset = current.extent as usize * BLOCK + position;
                let entry = record(&self.bytes, offset)?;
                let name = file_name(&self.bytes[offset..offset + size], joliet)?;
                if name.eq_ignore_ascii_case(component) {
                    if found.is_some() {
                        return Err("Duplicate ISO path".into());
                    }
                    found = Some(entry);
                }
                position += size;
            }
            current = found.ok_or_else(|| format!("ISO entry is missing: {path}"))?;
        }
        Ok(current)
    }

    fn data(&self, item: Record) -> Result<&[u8], String> {
        let start = (item.extent as usize)
            .checked_mul(BLOCK)
            .ok_or("ISO extent overflow")?;
        let end = start
            .checked_add(item.size as usize)
            .ok_or("ISO size overflow")?;
        self.bytes
            .get(start..end)
            .ok_or_else(|| "ISO entry exceeds image size".into())
    }
}

fn platform(code: u8) -> &'static str {
    if code == 0xef { "UEFI" } else { "BIOS" }
}
fn le16(input: &[u8]) -> u16 {
    u16::from_le_bytes(input.try_into().expect("two bytes"))
}
fn be16(input: &[u8]) -> u16 {
    u16::from_be_bytes(input.try_into().expect("two bytes"))
}
fn le32(input: &[u8]) -> u32 {
    u32::from_le_bytes(input.try_into().expect("four bytes"))
}
fn both32(input: &[u8]) -> Result<u32, String> {
    let le = le32(&input[..4]);
    if le != u32::from_be_bytes(input[4..8].try_into().expect("four bytes")) {
        return Err("ISO both-endian field differs".into());
    }
    Ok(le)
}
fn write_both32(output: &mut [u8], value: u32) {
    output[..4].copy_from_slice(&value.to_le_bytes());
    output[4..8].copy_from_slice(&value.to_be_bytes());
}
fn record(bytes: &[u8], offset: usize) -> Result<Record, String> {
    let size = *bytes.get(offset).ok_or("ISO directory record missing")? as usize;
    if size < 34 || offset % BLOCK + size > BLOCK || offset + size > bytes.len() {
        return Err("Invalid ISO directory record".into());
    }
    let raw = &bytes[offset..offset + size];
    let id_size = raw[32] as usize;
    if 33 + id_size > size || raw[1] != 0 || raw[26] != 0 || raw[27] != 0 {
        return Err("Unsupported ISO directory record".into());
    }
    Ok(Record {
        offset,
        extent: both32(&raw[2..10])?,
        size: both32(&raw[10..18])?,
        flags: raw[25],
    })
}
fn file_name(raw: &[u8], joliet: bool) -> Result<String, String> {
    let size = raw[32] as usize;
    let name = &raw[33..33 + size];
    if size == 1 && name[0] <= 1 {
        return Ok(if name[0] == 0 { "." } else { ".." }.into());
    }
    let value = if joliet {
        if size % 2 != 0 {
            return Err("Invalid Joliet file name".into());
        }
        let units = name
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        String::from_utf16(&units).map_err(|_| "Invalid Joliet text")?
    } else {
        String::from_utf8(name.to_vec()).map_err(|_| "Invalid ISO file name")?
    };
    Ok(value
        .split(';')
        .next()
        .unwrap_or("")
        .trim_end_matches('.')
        .to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_images_are_rejected() {
        assert!(Image::open(vec![0; 17 * BLOCK]).is_err());
        let mut hybrid = vec![0; 18 * BLOCK];
        hybrid[510..512].copy_from_slice(&[0x55, 0xaa]);
        assert!(Image::open(hybrid).is_err());
    }

    #[test]
    fn optical_fixture_keeps_boot_entries_and_kernel() {
        let Some(path) = std::env::var_os("CRISPAWN_OPTICAL_ISO_FIXTURE") else {
            return;
        };
        let mut image = Image::open(std::fs::read(path).unwrap()).unwrap();
        let platforms = image.boot_platforms().unwrap();
        assert_eq!(platforms, ["BIOS", "UEFI"]);
        let kernel = image.read_file("boot/bzImage").unwrap().to_vec();
        let initrd = image.read_file("boot/initrd").unwrap().to_vec();
        image
            .replace_file("boot/initrd", &initrd, 100_000_000)
            .unwrap();
        assert_eq!(image.boot_platforms().unwrap(), platforms);
        assert_eq!(image.read_file("boot/bzImage").unwrap(), kernel);
        assert_eq!(image.read_file("boot/initrd").unwrap(), initrd);
    }
}
