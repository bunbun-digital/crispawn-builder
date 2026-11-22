# Contributing

Thank you for contributing to CRISPAWN Builder. This repository contains the Windows community edition for local USB backups.

## Development

See [BUILDING.md](BUILDING.md) for the Windows build and verification commands.

1. Open an issue before a large change so the intended behavior and compatibility can be discussed.
2. Keep the Rust core, Tauri shell, and React interface separate. Use the same core for GUI and CLI operations.
3. Run `npm run build` and `cargo +stable test --locked -p crispawn-builder-core` on Windows. Build the desktop application when changing its commands or capabilities.
4. Include evidence for any claim about real USB writes, Secure Boot, or completed backups. Fixture tests alone do not establish those results.

Please do not commit ISOs, built executables, disk images, credentials, signing keys, or machine specific logs. The community edition does not accept S3/SMB delivery, Wi-Fi credential configuration, or driver download/injection code. Those features belong in the separate Pro product.

By submitting code, you agree that your contribution is licensed under Apache-2.0. The CRISPAWN artwork and name have separate terms in `assets/crispawn/LICENSE`; do not replace or modify them without permission from their owner.
