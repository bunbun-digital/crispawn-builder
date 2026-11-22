// Copyright (c) 2026 bunbun-digital and contributors. SPDX-License-Identifier: Apache-2.0
//! Bounded, read-only access to the signed EFI files in an optical FAT12 image.

const INVALID: &str = "Invalid FAT12 EFI image in the ISO.";
const DIR_LIMIT: usize = 64 * 1024;

struct Fat12<'a> {
    image: &'a [u8],
    fat_start: usize,
    fat_end: usize,
    root_start: usize,
    root_end: usize,
    data_start: usize,
    cluster_bytes: usize,
    cluster_count: usize,
}

struct Entry {
    cluster: usize,
    size: usize,
}

impl<'a> Fat12<'a> {
    fn open(image: &'a [u8]) -> Result<Self, String> {
        if image.len() < 512 || image[510..512] != [0x55, 0xaa] {
            return Err(INVALID.into());
        }
        let sector_bytes = usize::from(u16::from_le_bytes([image[11], image[12]]));
        let sectors_per_cluster = usize::from(image[13]);
        let reserved = usize::from(u16::from_le_bytes([image[14], image[15]]));
        let fats = usize::from(image[16]);
        let root_entries = usize::from(u16::from_le_bytes([image[17], image[18]]));
        let total16 = usize::from(u16::from_le_bytes([image[19], image[20]]));
        let fat_sectors = usize::from(u16::from_le_bytes([image[22], image[23]]));
        let total32 = u32::from_le_bytes([image[32], image[33], image[34], image[35]]) as usize;
        let total_sectors = if total16 != 0 { total16 } else { total32 };
        if !matches!(sector_bytes, 512 | 1024 | 2048 | 4096)
            || !sectors_per_cluster.is_power_of_two()
            || sectors_per_cluster > 128
            || reserved == 0
            || !(1..=2).contains(&fats)
            || root_entries == 0
            || fat_sectors == 0
        {
            return Err(INVALID.into());
        }
        let cluster_bytes = sector_bytes
            .checked_mul(sectors_per_cluster)
            .ok_or(INVALID)?;
        let root_bytes = root_entries.checked_mul(32).ok_or(INVALID)?;
        let root_sectors = root_bytes.div_ceil(sector_bytes);
        let fat_span = fats.checked_mul(fat_sectors).ok_or(INVALID)?;
        let root_sector = reserved.checked_add(fat_span).ok_or(INVALID)?;
        let data_sector = root_sector.checked_add(root_sectors).ok_or(INVALID)?;
        let image_bytes = total_sectors.checked_mul(sector_bytes).ok_or(INVALID)?;
        if total_sectors <= data_sector || image_bytes > image.len() {
            return Err(INVALID.into());
        }
        let cluster_count = (total_sectors - data_sector) / sectors_per_cluster;
        if !(1..4085).contains(&cluster_count) {
            return Err("The embedded EFI image is not FAT12.".into());
        }
        let fat_start = reserved.checked_mul(sector_bytes).ok_or(INVALID)?;
        let fat_end = fat_start
            .checked_add(fat_sectors.checked_mul(sector_bytes).ok_or(INVALID)?)
            .ok_or(INVALID)?;
        let root_start = root_sector.checked_mul(sector_bytes).ok_or(INVALID)?;
        let root_end = root_start.checked_add(root_bytes).ok_or(INVALID)?;
        let data_start = data_sector.checked_mul(sector_bytes).ok_or(INVALID)?;
        if fat_end > root_start
            || root_end > data_start
            || data_start > image_bytes
            || fat_end - fat_start < (cluster_count + 2) * 3 / 2 + 2
        {
            return Err(INVALID.into());
        }
        Ok(Self {
            image: &image[..image_bytes],
            fat_start,
            fat_end,
            root_start,
            root_end,
            data_start,
            cluster_bytes,
            cluster_count,
        })
    }

    fn find(&self, directory: &[u8], name: &[u8; 11], is_dir: bool) -> Result<Entry, String> {
        for item in directory.chunks_exact(32) {
            if item[0] == 0 {
                break;
            }
            if item[0] == 0xe5 || item[11] == 0x0f || item[11] & 0x08 != 0 {
                continue;
            }
            if &item[..11] == name && (item[11] & 0x10 != 0) == is_dir {
                return Ok(Entry {
                    cluster: usize::from(u16::from_le_bytes([item[26], item[27]])),
                    size: u32::from_le_bytes([item[28], item[29], item[30], item[31]]) as usize,
                });
            }
        }
        Err("Signed EFI file is missing from the ISO's FAT image.".into())
    }

    fn next(&self, cluster: usize) -> Result<Option<usize>, String> {
        let offset = self
            .fat_start
            .checked_add(cluster + cluster / 2)
            .ok_or(INVALID)?;
        let pair = self.image.get(offset..offset + 2).ok_or(INVALID)?;
        if offset + 2 > self.fat_end {
            return Err(INVALID.into());
        }
        let raw = u16::from_le_bytes([pair[0], pair[1]]);
        let next = usize::from(if cluster & 1 == 0 {
            raw & 0x0fff
        } else {
            raw >> 4
        });
        if next >= 0x0ff8 {
            return Ok(None);
        }
        if next < 2 || next >= self.cluster_count + 2 || next >= 0x0ff0 {
            return Err(INVALID.into());
        }
        Ok(Some(next))
    }

    fn read_chain(&self, first: usize, limit: usize) -> Result<Vec<u8>, String> {
        let mut cluster = first;
        let mut seen = vec![false; self.cluster_count + 2];
        let mut output = Vec::new();
        loop {
            if cluster < 2 || cluster >= seen.len() || seen[cluster] {
                return Err(INVALID.into());
            }
            seen[cluster] = true;
            let start = self
                .data_start
                .checked_add(
                    (cluster - 2)
                        .checked_mul(self.cluster_bytes)
                        .ok_or(INVALID)?,
                )
                .ok_or(INVALID)?;
            let end = start.checked_add(self.cluster_bytes).ok_or(INVALID)?;
            if output
                .len()
                .checked_add(self.cluster_bytes)
                .ok_or(INVALID)?
                > limit
            {
                return Err("Signed EFI file or directory exceeds its size limit.".into());
            }
            output.extend_from_slice(self.image.get(start..end).ok_or(INVALID)?);
            match self.next(cluster)? {
                Some(value) => cluster = value,
                None => return Ok(output),
            }
        }
    }
}

pub fn extract_boot_file(image: &[u8], name: &[u8; 11], limit: usize) -> Result<Vec<u8>, String> {
    let fat = Fat12::open(image)?;
    let root = &fat.image[fat.root_start..fat.root_end];
    let efi = fat.find(root, b"EFI        ", true)?;
    let efi_dir = fat.read_chain(efi.cluster, DIR_LIMIT)?;
    let boot = fat.find(&efi_dir, b"BOOT       ", true)?;
    let boot_dir = fat.read_chain(boot.cluster, DIR_LIMIT)?;
    let file = fat.find(&boot_dir, name, false)?;
    if file.size == 0 || file.size > limit {
        return Err("Signed EFI file exceeds its size limit.".into());
    }
    let padded = file
        .size
        .div_ceil(fat.cluster_bytes)
        .checked_mul(fat.cluster_bytes)
        .ok_or(INVALID)?;
    let mut data = fat.read_chain(file.cluster, padded)?;
    if data.len() != padded {
        return Err(INVALID.into());
    }
    data.truncate(file.size);
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::extract_boot_file;

    #[test]
    fn follows_a_fat12_file_across_two_clusters_and_rejects_a_cycle() {
        let mut image = vec![0u8; 8 * 512];
        image[11..13].copy_from_slice(&512u16.to_le_bytes());
        image[13] = 1;
        image[14..16].copy_from_slice(&1u16.to_le_bytes());
        image[16] = 1;
        image[17..19].copy_from_slice(&16u16.to_le_bytes());
        image[19..21].copy_from_slice(&8u16.to_le_bytes());
        image[22..24].copy_from_slice(&1u16.to_le_bytes());
        image[510..512].copy_from_slice(&[0x55, 0xaa]);
        let fat = &mut image[512..1024];
        fat[..3].copy_from_slice(&[0xf8, 0xff, 0xff]);
        for (cluster, next) in [(2, 0xfff), (3, 0xfff), (4, 5), (5, 0xfff)] {
            let offset = cluster * 3 / 2;
            let raw = u16::from_le_bytes([fat[offset], fat[offset + 1]]);
            let value = if cluster & 1 == 0 {
                (raw & 0xf000) | next
            } else {
                (raw & 0x000f) | (next << 4)
            };
            fat[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
        for (sector, name, cluster, size, attr) in [
            (2, b"EFI        ", 2u16, 0u32, 0x10u8),
            (3, b"BOOT       ", 3u16, 0u32, 0x10u8),
            (4, b"BOOTX64 EFI", 4u16, 600u32, 0x20u8),
        ] {
            let start = sector * 512;
            image[start..start + 11].copy_from_slice(name);
            image[start + 11] = attr;
            image[start + 26..start + 28].copy_from_slice(&cluster.to_le_bytes());
            image[start + 28..start + 32].copy_from_slice(&size.to_le_bytes());
        }
        image[5 * 512..6 * 512].fill(b'A');
        image[6 * 512..6 * 512 + 88].fill(b'B');
        let data = extract_boot_file(&image, b"BOOTX64 EFI", 1024).unwrap();
        assert_eq!(data.len(), 600);
        assert!(data[..512].iter().all(|byte| *byte == b'A'));
        assert!(data[512..].iter().all(|byte| *byte == b'B'));
        image[512 + 4 * 3 / 2..512 + 4 * 3 / 2 + 2].copy_from_slice(&4u16.to_le_bytes());
        assert!(extract_boot_file(&image, b"BOOTX64 EFI", 1024).is_err());
    }
}
