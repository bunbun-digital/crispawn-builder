# Build CRISPAWN Builder

This guide is for developers building the Windows community edition from source. For normal operation, see [README.md](README.md). Building from source requires developer tools; running the resulting application does not require Python or xorriso.

## Requirements

- Windows with WebView2
- Git, Node.js and npm
- Rust with the MSVC target
- Microsoft C++ Build Tools and Windows SDK

Dependency versions are pinned in `Cargo.lock` and `package-lock.json`. The LiveOS ISO and Debian signed boot files are separate inputs and are not included in this repository.

## Desktop application

```powershell
git clone https://github.com/bunbun-digital/crispawn-builder.git
Set-Location crispawn-builder
npm ci
npm run build
& "$env:USERPROFILE\.cargo\bin\cargo.exe" +stable build --locked -p crispawn-builder-desktop --features custom-protocol
```

The executable is `build/rust/debug/crispawn-builder-desktop.exe`. The interface can be developed with `npm run tauri dev`. Build outputs are ignored by Git.

## Command line tool

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" +stable build --locked -p crispawn-builder-core
& .\build\rust\debug\crispawn-builder.exe --help
```

The CLI uses the same Rust controller as the desktop application. `iso BASE.iso OUTPUT.iso BASE_SHA256` creates a configured ISO and its `.sha256` and `.json` sidecars. `plan` previews legacy LiveOS settings; `usb-list` and `disk-list` enumerate devices; `efi-extract` extracts signed boot files from a compatible ISO. The `usb` command requires the selected disk's number, unique ID, and size and revalidates its identity before writing. Use the desktop application for normal USB creation.

ISO configuration is implemented in Rust. It reads the ISO9660 image, updates `/boot/initrd`, reads the settings back, and checks that signed boot files and boot entries are unchanged. No external Python, xorriso, or helper executable is invoked.

## Verification

```powershell
npm run build
& "$env:USERPROFILE\.cargo\bin\cargo.exe" +stable test --locked -p crispawn-builder-core
& "$env:USERPROFILE\.cargo\bin\cargo.exe" +stable build --locked -p crispawn-builder-desktop --features custom-protocol
```

Tests cover settings validation, ISO and initramfs handling, USB layout checks, FAT extraction, and rejection of unknown ISOs. A build or fixture test does not establish that a physical USB boots or completes a backup. Record real hardware acceptance separately.

## Repository layout

| Path | Purpose |
| --- | --- |
| `.cargo/config.toml` | Keeps Rust build output under the ignored `build/rust/` directory |
| `ui/` | Vite entry point, React interface, and language strings |
| `src-tauri/` | Tauri desktop shell and Windows dialogs |
| `crates/builder-core/` | Settings, ISO, EFI, and USB controllers plus CLI |
| `config/crispawn/` | Example LiveOS settings |
| `data/` | Pinned development boot metadata |
| `assets/crispawn/` | Separately licensed CRISPAWN artwork and screenshots |

The source code is licensed under [Apache-2.0](LICENSE). The name and artwork have [separate asset terms](assets/crispawn/LICENSE). See [CONTRIBUTING.md](CONTRIBUTING.md) for contribution guidance.
