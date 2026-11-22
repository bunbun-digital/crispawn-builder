# CRISPAWN Builder

[English README](README.md)

CRISPAWN Builderは、WindowsでCRISPAWN LiveOSの起動用USBを作るデスクトップアプリです。利用者が選んだISOの中から署名済み起動ファイルを取り出し、準備済みUSBへ書き込みます。ISOの設定を変更するCLIも同じRustの処理を使用します。

この公開版は開発段階です。LiveOSのISO、Windowsインストーラー、ISOの自動ダウンロード機能は含みません。

## OSS版の範囲

- コピー元、保存先、確認、書き込み中、完了の画面を提供します。
- Windows上の内蔵ディスクと準備済みUSBを一覧にし、USBのドライブレターも表示します。
- ISO内のCRISPAWN媒体設定、カーネル、Debian署名済みEFIファイルを確認します。
- USB上のISOと起動ファイルを書き込み、読み戻して検証します。NTFS保存領域は保持します。
- 対応するベースISOに設定を組み込み、`live.json`と起動ファイルの状態を検証できます。

**保存先はUSBのみです。** 生成する設定では`destination.kind`は`usb_disk`、`network.mode`は`none`です。S3・SMBへの送信、Wi‑Fi設定、ドライバーのダウンロードと組み込みはありません。これらは別製品のCRISPAWN Pro Builderで検討します。OSS版はカーネルやWi‑Fiドライバーを同梱・再配布しません。

## USB作成に必要なもの

Windows、対応するCRISPAWN LiveOSのISO、準備済みUSBが必要です。USBはGPT形式で、**256 MiBのEFIシステムパーティション**と、ラベルが`CRISPAWN`の**NTFSパーティション**の2つを持つ必要があります。空のUSBを初期化する機能はありません。

ISOは100,000,000バイト未満で、`/boot/crispawn-media.json`を含み、[`data/debian-base-boot.json`](data/debian-base-boot.json)に記録された開発用リリースのカーネル・EFIファイルと一致する必要があります。BuilderはISOやEFIファイルをネットから取得しません。

ベースISOからの**設定入りISO生成**は、デスクトップアプリとCLIだけで実行できます。Python、xorriso、補助EXEのインストールや環境変数の設定は不要です。対応するベースISOを選び、信頼できる配布元で確認したSHA-256を入力してください。

runtime v2のベースISOには、initramfs内の`/etc/perfect-backup/release-capabilities.json`に対応設定の宣言が必要です。このファイルがないv2のISOは拒否します。ISOの配布元で宣言を含めて生成する必要があります。

## ソースからビルド

Git、Node.js/npm、MSVC版Rust、Microsoft C++ Build Tools、Windows SDK、WebView2を用意します。

```powershell
git clone https://github.com/bunbun-digital/crispawn-builder.git
Set-Location crispawn-builder
npm ci
npm run build
& "$env:USERPROFILE\.cargo\bin\cargo.exe" +stable build --locked -p crispawn-builder-desktop --features custom-protocol
```

実行ファイルは`build/rust/debug/crispawn-builder-desktop.exe`に生成されます。画面の開発には`npm run tauri dev`を使えます。CLIは次のコマンドで作成できます。

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" +stable build --locked -p crispawn-builder-core
& .\build\rust\debug\crispawn-builder.exe --help
```

## GUIでの手順

1. 使用するPCを選びます。現行の対応ISOは全内蔵ディスクのコピーに対応し、一部のディスクだけを選ぶ設定には対応していません。
2. USB保存先と、LiveOS起動後・完了後の動作を確認します。
3. 準備済みUSBを選び、表示される画面でISOを指定します。
4. 書き込み確認とWindowsの権限確認を行います。進捗画面で書き込みと読み戻し検証を確認します。
5. 完了画面を確認してからUSBを取り外します。実機での起動とバックアップは別途確認してください。

書き込み先はディスク番号・固有ID・容量を再照合します。ドライブレターだけを頼りにディスクを選ばないでください。

## 試験

```powershell
npm ci
npm run build
& "$env:USERPROFILE\.cargo\bin\cargo.exe" +stable test --locked -p crispawn-builder-core
& "$env:USERPROFILE\.cargo\bin\cargo.exe" +stable build --locked -p crispawn-builder-desktop --features custom-protocol
```

単体テストとビルドの成功は、USB実機での起動やバックアップ完了を意味しません。実機結果は分けて記録してください。

## ライセンスと貢献

ソースコードは[Apache-2.0](LICENSE)です。貢献手順は[CONTRIBUTING.md](CONTRIBUTING.md)を参照してください。

CRISPAWNの名称・クリスタル・文字ロゴ・アプリアイコンは別ライセンスの製品資産です。[`assets/crispawn/LICENSE`](assets/crispawn/LICENSE)と[NOTICE](NOTICE)を参照してください。LiveOSのISOとDebianの署名済み起動ファイルは、このリポジトリには含みません。
