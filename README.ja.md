<p align="center">
  <img src="assets/crispawn/ui/logo-hero.png" alt="CRISPAWN" width="860">
</p>

# CRISPAWN Builder

CRISPAWN BuilderはWindowsでCRISPAWN LiveOSの起動用USBを作るアプリです。ベースISOに設定を組み込み、新しいISOも作れます。OSS版のバックアップ保存先はUSBです。

[English](README.md)

![CRISPAWN Builderのコピー保存先画面](assets/crispawn/screenshots/gui-destination-ja.png)

*日本語表示のコピー保存先画面。*

## 用意するもの

- Windows PCとインターネット接続。BuilderがGitHubのCRISPAWN公開リリースを確認してISOを取得します。
- WindowsでUSB接続ディスクとして認識される、512 MiB以上のUSBメモリまたはUSBハードディスク・SSD。

取得したISOはPC内に保存されます。同じリリースは再利用し、新しいリリースが公開されたときだけダウンロードします。このソースリポジトリにインストーラーとLiveOSのISOは含まれていません。

## 起動用USBを作る

1. CRISPAWN Builderを起動します。**このPC**を選ぶと内蔵ディスクが表示されます。**他のPC**を選ぶと、起動先の全内蔵ディスクを対象にします。現行の対応ISOでは、一部のディスクだけを選んでコピーすることはできません。
2. コピー保存先が**USBハードディスク**であることを確認します。
3. **確認**画面でバックアップ開始までの待ち時間、LiveOSの言語、完了後の動作を選びます。作成するものを**起動用USB**とし、名称・容量・ディスク番号・ドライブレターを見てUSB接続ディスクを選びます。未準備のディスクは「初期化が必要」と表示されます。
4. **USBに書き込む**を押し、対象ディスクと操作内容を確認します。初期化が必要な場合、そのディスクの全パーティションとデータは削除されます。Windowsの確認画面を承認し、読み戻し照合が終わるまでUSBを抜かないでください。
5. **完了**画面が出たらUSBを取り外します。対象PCで起動し、バックアップ結果を確認してください。

準備済みのCRISPAWN USBでは既存NTFS保存領域を保持します。未準備のUSBはGPT、256 MiBのEFIシステム領域、残りのNTFS保存領域として初期化します。Builderは書き込み直前にUSBの識別情報と構成を再照合します。

## 設定入りISOを作る

1. **コピー対象**と**コピー保存先**を選びます。
2. **確認**画面の「作成するもの」を**ISOファイル**にします。
3. **ISOを作成**を押し、新しいISOの保存先を選びます。BuilderがGitHubの最新ISOを確認します。**完了**画面が出るまで待ちます。

ISOとともに照合用の`.sha256`と`.json`が生成されます。ISO生成はアプリ内で完結し、Python、xorriso、補助プログラムのインストールは不要です。

runtime v2のベースISOから設定入りISOを作るには、対応設定を宣言する`/etc/perfect-backup/release-capabilities.json`が必要です。このファイルがないというエラーが出た場合は、配布元から修正版のベースISOを入手してください。

OSS版のバックアップ保存先はローカルUSBです。SMB、S3、Wi-Fi設定、ドライバー組み込みには対応していません。

ソースからのビルドとテストは[BUILDING.md](BUILDING.md)を参照してください。コードは[Apache-2.0](LICENSE)、ロゴとスクリーンショットは[別ライセンス](assets/crispawn/LICENSE)です。
