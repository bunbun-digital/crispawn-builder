# CRISPAWN Builder

<img src="assets/crispawn/ui/wordmark.png" alt="CRISPAWN" width="280">

**CRISPAWN Builder** is a Windows desktop application for preparing a bootable CRISPAWN LiveOS USB drive. It takes a compatible LiveOS ISO selected by the user, extracts the signed boot files already inside that ISO, and writes them to a prepared USB drive. A command line tool uses the same Rust core for inspection and image creation.

[日本語のREADME](README.ja.md)

> This repository is an early public source release. It does not include a LiveOS ISO, a Windows installer, or a service that downloads an ISO. The USB writer currently requires a drive with the CRISPAWN partition layout described below.

## What it does

- Guides you through the copy source, destination, review, writing, and completion screens.
- Lists internal disks and prepared USB drives, including their Windows drive letters.
- Uses a user selected ISO and checks its CRISPAWN media layout, kernel, signed Debian EFI files, and SHA-256 before a USB write.
- Updates the boot files and ISO on an existing CRISPAWN USB drive, then reads back the result for verification. The NTFS data partition is retained.
- Can make a settings customized ISO from a compatible base ISO. The writer verifies that the initramfs contains the requested `live.json` and that the signed boot components have not changed.
- Offers optional Hyper-V boot testing when a suitable local test setup is available.

The community edition targets **local USB backup**. Its generated settings use `destination.kind = usb_disk` and `network.mode = none`. It has no S3 or SMB destination, Wi-Fi configuration UI, driver download, or driver injection. Features of that kind are planned for the separate CRISPAWN Pro Builder. The community source does not bundle or redistribute kernel or Wi-Fi drivers.

## Before you start

You need Windows, a compatible CRISPAWN LiveOS ISO, and a USB drive that has already been prepared as GPT with **exactly two partitions**: a 256 MiB EFI System Partition and an NTFS data partition labeled `CRISPAWN`. The USB drive must be reported by Windows as a USB disk. The Builder does not format a blank drive. Back up any important data before preparing a drive outside this application.

The selected ISO must be smaller than 100,000,000 bytes, contain `/boot/crispawn-media.json` with the CRISPAWN data layout, and contain the kernel and Debian signed EFI files matching the pinned development release metadata in [`data/debian-base-boot.json`](data/debian-base-boot.json). This pin is a compatibility and integrity check for the currently supported release, not a general signature verification service. The Builder does not download the ISO or its boot files.

The desktop app and CLI create a configured ISO directly. No Python, xorriso, helper executable, or environment variable is required at runtime. Choose a compatible base ISO and provide its SHA-256 from a trusted source.

A base ISO that uses runtime v2 must include `/etc/perfect-backup/release-capabilities.json` in its initramfs and declare support for the requested settings. The Builder rejects a v2 base without this release metadata; the ISO producer must add it to the release.

## Build from source

Development prerequisites: Git, Node.js and npm, Rust with the MSVC toolchain, Microsoft C++ Build Tools and Windows SDK, and WebView2. The dependency versions are pinned in `Cargo.lock` and `package-lock.json`.

```powershell
git clone https://github.com/bunbun-digital/crispawn-builder.git
Set-Location crispawn-builder
npm ci
npm run build
& "$env:USERPROFILE\.cargo\bin\cargo.exe" +stable build --locked -p crispawn-builder-desktop --features custom-protocol
```

The desktop executable is generated at `build/rust/debug/crispawn-builder-desktop.exe`. A standalone installer is not yet provided. To work on the interface, run `npm run tauri dev` after installing dependencies. Build outputs are ignored by Git.

For the command line tool:

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" +stable build --locked -p crispawn-builder-core
& .\build\rust\debug\crispawn-builder.exe --help
```

`plan` previews the legacy LiveOS configuration. `iso BASE.iso OUTPUT.iso BASE_SHA256` creates a configured ISO and its `.sha256` and `.json` sidecars. `usb-list` and `disk-list` only enumerate devices. `efi-extract` reads signed boot files from a compatible ISO. The `usb` command requires the selected disk's number, unique ID, and size and revalidates its identity before writing; use the desktop interface for normal USB creation.

## How the desktop flow works

1. Choose **This PC** or **Another PC**. The current compatible ISO copies all internal disks; choosing a subset is shown in the UI but cannot be represented by this ISO release. For another PC, a generic all internal disks setting is used.
2. Review the local USB destination and the LiveOS start and completion behavior.
3. Select a prepared USB drive. The Builder shows its disk number, capacity, and drive letter. Select the compatible ISO when prompted.
4. Review the explicit write confirmation and Windows elevation prompt. The progress screen reports input checks, writing, and readback verification.
5. Wait for the completion screen before removing the drive. Boot and backup behavior must still be tested on the target PC.

The USB flow preserves the existing NTFS data partition. It replaces the ISO and boot files on the selected prepared drive. The application validates the target again immediately before the elevated write. Never select a disk by drive letter alone.

## Verification

```powershell
npm ci
npm run build
& "$env:USERPROFILE\.cargo\bin\cargo.exe" +stable test --locked -p crispawn-builder-core
& "$env:USERPROFILE\.cargo\bin\cargo.exe" +stable build --locked -p crispawn-builder-desktop --features custom-protocol
```

The core tests cover settings validation, USB layout checks, FAT extraction, and rejection of unknown ISOs. A successful build or fixture test does not prove that a USB drive boots, that Secure Boot accepts a particular machine, or that a backup completed. Record those results separately when testing real hardware.

## Repository layout

| Path | Purpose |
| --- | --- |
| `ui/` | React user interface and language strings |
| `src-tauri/` | Tauri desktop shell and Windows dialogs |
| `crates/builder-core/` | Settings, ISO, EFI, USB, and Hyper-V controllers plus CLI |
| `crates/builder-core/src/image.rs` | Built-in ISO settings writer |
| `config/crispawn/` | Example LiveOS settings used by the Builder |
| `data/` | Pinned development boot metadata |
| `assets/crispawn/` | Separately licensed CRISPAWN product artwork |

## Contributing and licenses

Source code in this repository is licensed under [Apache License 2.0](LICENSE). See [CONTRIBUTING.md](CONTRIBUTING.md) for development guidance.

The CRISPAWN name, crystal, wordmark, and application icon are included under the **separate asset terms** in [`assets/crispawn/LICENSE`](assets/crispawn/LICENSE). They are not covered by Apache-2.0. See [NOTICE](NOTICE) for the distinction. Third-party packages retain their own licenses. A LiveOS ISO and Debian signed boot files are not distributed in this repository.
