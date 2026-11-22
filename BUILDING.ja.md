# CRISPAWN Builderのビルド

[English](BUILDING.md)

Windows向けOSS版をソースからビルドする開発者用の手順です。アプリの使い方は[README.ja.md](README.ja.md)を参照してください。開発用ツールはビルド時に必要ですが、完成したアプリでISOを生成する際にPythonやxorrisoは不要です。

## 開発環境

- WebView2を利用できるWindows
- Git、Node.js、npm
- MSVCターゲットのRust
- Microsoft C++ Build Tools、Windows SDK

依存の版は`Cargo.lock`と`package-lock.json`で固定しています。LiveOSのISOとDebian署名済み起動ファイルは別途用意する入力で、このリポジトリには含みません。

## デスクトップアプリ

```powershell
git clone https://github.com/bunbun-digital/crispawn-builder.git
Set-Location crispawn-builder
npm ci
npm run build
& "$env:USERPROFILE\.cargo\bin\cargo.exe" +stable build --locked -p crispawn-builder-desktop --features custom-protocol
```

実行ファイルは`build/rust/debug/crispawn-builder-desktop.exe`です。画面の開発には`npm run tauri dev`を使えます。ビルド成果物はGit管理から除外されます。

## CLI

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" +stable build --locked -p crispawn-builder-core
& .\build\rust\debug\crispawn-builder.exe --help
```

CLIとGUIは同じRustの処理を使用します。`iso BASE.iso OUTPUT.iso BASE_SHA256`は設定入りISOと`.sha256`・`.json`を生成します。`plan`は旧形式設定の確認、`usb-list`と`disk-list`は機器の列挙、`efi-extract`はISO内の署名済み起動ファイルの取り出しに使います。`usb`コマンドでは対象ディスクの番号・固有ID・容量を指定し、書き込み直前に再照合します。通常のUSB作成にはGUIを使ってください。

ISO設定はRust内でISO9660を読み込み、`/boot/initrd`を更新します。設定の読み戻しと署名付き起動ファイル・起動情報の不変性を確認し、外部のPython、xorriso、補助EXEは起動しません。

## 検証

```powershell
npm run build
& "$env:USERPROFILE\.cargo\bin\cargo.exe" +stable test --locked -p crispawn-builder-core
& "$env:USERPROFILE\.cargo\bin\cargo.exe" +stable build --locked -p crispawn-builder-desktop --features custom-protocol
```

テストは設定、ISO・initramfs、USBレイアウト、FAT展開、未知ISOの拒否を確認します。ビルドやfixtureの成功は、物理USBでの起動やバックアップ完了を証明しません。Hyper-Vと実機の結果は別々に記録してください。

## ディレクトリ

| パス | 内容 |
| --- | --- |
| `ui/` | React画面と表示文 |
| `src-tauri/` | TauriアプリとWindowsダイアログ |
| `crates/builder-core/` | 設定、ISO、EFI、USB、Hyper-Vの処理とCLI |
| `config/crispawn/` | LiveOS設定例 |
| `data/` | 開発用の起動メタデータ |
| `assets/crispawn/` | 別ライセンスのロゴとスクリーンショット |

ソースコードは[Apache-2.0](LICENSE)です。名称と画像には[別ライセンス](assets/crispawn/LICENSE)が適用されます。貢献方法は[CONTRIBUTING.md](CONTRIBUTING.md)を参照してください。
