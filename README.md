<p align="center">
  <img src="assets/crispawn/ui/logo-hero.png" alt="CRISPAWN" width="860">
</p>

# CRISPAWN Builder

CRISPAWN Builder creates bootable CRISPAWN LiveOS USB drives on Windows. It can also create an ISO with your selected LiveOS settings. The community edition saves backups to the USB drive.

[日本語で読む](README.ja.md)

![CRISPAWN Builder Copy destination screen](assets/crispawn/screenshots/gui-destination-en.png)

*Copy destination screen in English.*

## What you need

- A Windows PC and a compatible CRISPAWN LiveOS ISO.
- For USB creation, a USB drive already prepared with GPT, a 256 MiB EFI System Partition, and an NTFS data partition labeled `CRISPAWN`. The drive must appear in Windows as a USB disk.
- For creating a configured ISO, the base ISO's SHA-256 value from its trusted distributor.

The Builder does not format a blank USB drive or download the ISO. This source repository does not currently include an installer or LiveOS ISO.

## Create a bootable USB drive

1. Open CRISPAWN Builder. Choose **This PC** to see its internal disks, or **Another PC** for a generic all internal disks setting. The currently supported ISO copies all internal disks; selecting only some disks is unavailable.
2. Review the **USB hard drive** backup destination.
3. On **Review**, check the start delay, LiveOS language, and completion action. Choose **Bootable USB** and select the prepared USB drive by its name, size, disk number, and drive letter.
4. Click **Write USB**, select the compatible ISO, and review the Windows confirmation. Keep the USB connected while the progress screen checks, writes, and reads back the files.
5. Wait for **Complete** before removing the drive. Boot the USB on the target PC and check the backup result there.

The existing NTFS data partition is retained. The Builder replaces the ISO and boot files on the selected USB drive and checks the disk identity again before writing.

## Create an ISO with settings

1. Complete **Copy sources** and **Copy destination**.
2. On **Review**, choose **ISO file** under **Output** and enter the base ISO's SHA-256 from its distributor.
3. Click **Create ISO**, choose the base ISO and a new output path, then wait for **Complete**.

The output includes the ISO and `.sha256` and `.json` verification files. ISO creation runs inside the application; Python, xorriso, and extra helper programs are not needed.

A runtime v2 base ISO must declare the settings it supports in `/etc/perfect-backup/release-capabilities.json` inside its initramfs. If the Builder reports that this release metadata is missing, obtain a corrected base ISO from its distributor.

The community edition supports the local USB backup destination. SMB, S3, Wi-Fi settings, and driver installation are unavailable.

For source builds and tests, see [BUILDING.md](BUILDING.md). The code is [Apache-2.0](LICENSE); the logo and screenshots have [separate asset terms](assets/crispawn/LICENSE).
