<p align="center">
  <img src="assets/crispawn/ui/logo-hero.png" alt="CRISPAWN" width="860">
</p>

# CRISPAWN Builder

CRISPAWN Builder creates bootable CRISPAWN LiveOS USB drives on Windows. It can also embed your settings in a base ISO to create a new ISO. The community edition saves backups to the USB drive.

[日本語で読む](README.ja.md)

![CRISPAWN Builder Copy destination screen](assets/crispawn/screenshots/gui-destination-en.png)

*Copy destination screen in English.*

## What you need

- A Windows PC and a compatible CRISPAWN LiveOS ISO.
- For USB creation, a prepared GPT USB drive. It needs a 256 MiB EFI System Partition and an NTFS data partition labeled `CRISPAWN`. Windows must recognize it as a USB disk.
- For creating a configured ISO, the base ISO's SHA-256 value from its trusted distributor.

The Builder does not format a blank USB drive or download an ISO. This source repository does not include an installer or LiveOS ISO.

## Create a bootable USB drive

1. Open CRISPAWN Builder. Choose **This PC** to see its internal disks, or **Another PC** to target every internal disk on the PC where LiveOS boots. With the currently supported ISO, you cannot copy only some disks.
2. Review the **USB hard drive** backup destination.
3. On **Review**, choose the delay before backup starts, the LiveOS language, and what happens after completion. Choose **Bootable USB** and select the prepared USB drive by its name, size, disk number, and drive letter.
4. Click **Write USB**, select the compatible ISO, and review the Windows confirmation. Keep the USB connected until writing and readback verification finish on the progress screen.
5. When **Complete** appears, remove the USB drive. Boot it on the target PC and check the backup result.

The Builder retains the existing NTFS data partition. It writes the selected ISO, boot files, and settings chosen in the interface to the boot partition, then reads them back to verify them. It also checks the USB disk identity again just before writing.

## Create an ISO with settings

1. Complete **Copy sources** and **Copy destination**.
2. On **Review**, choose **ISO file** under **Output** and enter the base ISO's SHA-256 from its distributor.
3. Click **Create ISO**, choose the base ISO and a new output path, then wait for **Complete**.

The output includes the ISO and `.sha256` and `.json` verification files. ISO creation runs inside the application; Python, xorriso, and extra helper programs are not needed.

To create an ISO with settings from a runtime v2 base ISO, the base must include `/etc/perfect-backup/release-capabilities.json`, which declares its supported settings. If the Builder reports that this file is missing, obtain a corrected base ISO from its distributor.

The community edition supports the local USB backup destination. SMB, S3, Wi-Fi settings, and driver installation are unavailable.

For source builds and tests, see [BUILDING.md](BUILDING.md). The code is [Apache-2.0](LICENSE); the logo and screenshots have [separate asset terms](assets/crispawn/LICENSE).
