# Build CRISPAWN Builder

This guide is for developers building the Windows community edition from source. For normal operation, see [README.md](README.md). Building from source requires developer tools; running the resulting application does not require Python or xorriso.

## Requirements

- Windows with WebView2
- Git, Node.js and npm
- Rust through rustup with the MSVC target. `rust-toolchain.toml` pins the toolchain (currently 1.100.0) so builds are reproducible; rustup installs and selects it automatically in this repository. Do not override it with `+stable` or another toolchain.
- Microsoft C++ Build Tools and Windows SDK

Dependency versions are pinned in `Cargo.lock` and `package-lock.json`. The desktop app downloads the newest published CRISPAWN community ISO from `bunbun-digital/crispawn` and caches it under `%LOCALAPPDATA%\CRISPAWN\Builder\iso-cache`.

## Desktop application

```powershell
git clone https://github.com/bunbun-digital/crispawn-builder.git
Set-Location crispawn-builder
npm ci
npm run build
& "$env:USERPROFILE\.cargo\bin\cargo.exe" build --locked -p crispawn-builder-desktop --features custom-protocol
```

The executable is `build/rust/debug/crispawn-builder-desktop.exe`. The interface can be developed with `npm run tauri dev`. Build outputs are ignored by Git.

## Command line tool

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" build --locked -p crispawn-builder-core
& .\build\rust\debug\crispawn-builder.exe --help
```

The CLI uses the same Rust controller as the desktop application. `release-iso` checks GitHub and downloads or reuses the latest ISO; `release-capabilities` reads that ISO's declared settings. The low-level `iso BASE.iso OUTPUT.iso BASE_SHA256` command creates a configured ISO and its `.sha256` and `.json` sidecars for development fixtures. `plan` previews legacy settings by default or runtime v2 settings when advanced flags are supplied. `usb-list` enumerates all USB bus disks, including unprepared flash drives, HDDs and SSDs; `disk-list` enumerates internal disks; `efi-extract` extracts pinned signed boot files from a compatible development ISO. The low-level `usb` command requires a prepared disk's number, unique ID, and size. Pass it the base ISO that `efi-extract` used, with that ISO's SHA-256; a configured ISO created by `iso` is rejected because its digest differs from the EFI bundle's. `REPORT_DIR` must be a path that does not exist yet. Use the desktop application for automatic initialization and normal USB creation.

The USB writer places the configured ISO at `/crispawn.iso` on the EFI partition. The release ISO must declare this path in `/boot/crispawn-media.json` using media layout schema v2. Backup data and logs use the `crispawn` directory on the NTFS partition. Keep both sides of this boot contract synchronized when releasing a new LiveOS image.

ISO configuration is implemented in Rust. It reads the ISO9660 image, updates `/boot/initrd`, reads the settings back, and checks that signed boot files and boot entries are unchanged. No external Python, xorriso, or helper executable is invoked.

## Verification

```powershell
npm run build
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test --locked -p crispawn-builder-core
& "$env:USERPROFILE\.cargo\bin\cargo.exe" build --locked -p crispawn-builder-desktop --features custom-protocol
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\tests\usb-writer-preflight.ps1
```

The USB writer preflight test also runs from a release package when `usb_writer.ps1` is placed beside it in `tests\`, or with `-WriterPath PATH`. It mocks every storage cmdlet and never touches a physical disk.

Tests cover settings validation, ISO and initramfs handling, USB layout checks, FAT extraction, and rejection of unknown ISOs. A build or fixture test does not establish that a physical USB boots or completes a backup. Record real hardware acceptance separately.

## Release package

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\package.ps1 [-Destination DIR] [-SignScript SIGN.ps1]
```

The script builds the UI and release executables, runs the core tests, and creates `crispawn-builder-oss-<UTC>-<commit>\` with `manifest.json` (SHA-256 of every file) and `LATEST.json` under `build\package` or `-Destination`. The desktop executable embeds the UI, so no `ui\` folder is shipped. `tests\` contains the preflight test and `usb_writer.ps1`, and the script runs that test inside the package. `-SignScript` is called as `SIGN.ps1 -Path EXE` for each executable before hashing; unsigned executables produce a warning, and an invalid signature stops packaging. The working tree must be clean unless `-AllowDirty` is passed.

## Repository layout

| Path | Purpose |
| --- | --- |
| `.cargo/config.toml` | Keeps Rust build output under the ignored `build/rust/` directory |
| `scripts/package.ps1` | Builds, tests, and packages the release executables |
| `ui/` | Vite entry point, React interface, and language strings |
| `src-tauri/` | Tauri desktop shell and Windows dialogs |
| `crates/builder-core/` | Settings, ISO, EFI, and USB controllers plus CLI |
| `config/crispawn/` | Example LiveOS settings |
| `data/` | Pinned development boot metadata |
| `assets/crispawn/` | Separately licensed CRISPAWN artwork and screenshots |

The source code is licensed under [Apache-2.0](LICENSE). The name and artwork have [separate asset terms](assets/crispawn/LICENSE). See [CONTRIBUTING.md](CONTRIBUTING.md) for contribution guidance.
