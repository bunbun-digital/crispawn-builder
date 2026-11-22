<p align="center">
  <img src="assets/crispawn/ui/logo-hero.png" alt="CRISPAWN" width="860">
</p>

# CRISPAWN Builder

CRISPAWN BuilderはWindowsでCRISPAWN LiveOSの起動用USBを作るアプリです。ベースISOに設定を組み込み、新しいISOも作れます。OSS版のバックアップ保存先はUSBです。

[English](README.md)

![CRISPAWN Builderのコピー保存先画面](assets/crispawn/screenshots/gui-destination-ja.png)

*日本語表示のコピー保存先画面。*

## 用意するもの

- Windows PCと、対応するCRISPAWN LiveOSのISO。
- USB作成にはGPT形式の準備済みUSBが必要です。256 MiBのEFIシステムパーティションと、ラベルが`CRISPAWN`のNTFS保存領域を用意してください。WindowsでUSBディスクとして認識される必要があります。
- 設定入りISOの作成にはベースISOの配布元で確認したSHA-256値。

Builderは空のUSBを初期化したり、ISOをダウンロードしたりしません。このソースリポジトリにインストーラーとLiveOSのISOは含まれていません。

## 起動用USBを作る

1. CRISPAWN Builderを起動します。**このPC**を選ぶと内蔵ディスクが表示されます。**他のPC**を選ぶと、起動先の全内蔵ディスクを対象にします。現行の対応ISOでは、一部のディスクだけを選んでコピーすることはできません。
2. コピー保存先が**USBハードディスク**であることを確認します。
3. **確認**画面でバックアップ開始までの待ち時間、LiveOSの言語、完了後の動作を選びます。作成するものを**起動用USB**とし、名称・容量・ディスク番号・ドライブレターを見て準備済みUSBを選びます。
4. **USBに書き込む**を押して対応ISOを選び、Windowsの確認画面を確認します。進捗画面で書き込みと読み戻し照合が終わるまでUSBを抜かないでください。
5. **完了**画面が出たらUSBを取り外します。対象PCで起動し、バックアップ結果を確認してください。

BuilderはUSB上の既存NTFS保存領域を保持します。選択したISO、起動ファイル、画面で選んだ設定を起動領域に書き込み、読み戻して照合します。書き込み直前にはUSBの識別情報も再照合します。

## 設定入りISOを作る

1. **コピー対象**と**コピー保存先**を選びます。
2. **確認**画面の「作成するもの」を**ISOファイル**にし、配布元で確認したベースISOのSHA-256値を入力します。
3. **ISOを作成**を押し、ベースISOと新しい保存先を選びます。**完了**画面が出るまで待ちます。

ISOとともに照合用の`.sha256`と`.json`が生成されます。ISO生成はアプリ内で完結し、Python、xorriso、補助プログラムのインストールは不要です。

runtime v2のベースISOから設定入りISOを作るには、対応設定を宣言する`/etc/perfect-backup/release-capabilities.json`が必要です。このファイルがないというエラーが出た場合は、配布元から修正版のベースISOを入手してください。

OSS版のバックアップ保存先はローカルUSBです。SMB、S3、Wi-Fi設定、ドライバー組み込みには対応していません。

ソースからのビルドとテストは[BUILDING.md](BUILDING.md)を参照してください。コードは[Apache-2.0](LICENSE)、ロゴとスクリーンショットは[別ライセンス](assets/crispawn/LICENSE)です。
