---
name: 不具合の報告 / Bug report
about: 誤った文字の入力、キーの意図しない動作など / Wrong characters typed, keys acting unexpectedly, etc.
labels: bug
---

<!--
分かる範囲で記入する。Issue は公開されるので、設定やログのうち公開を望まない値（ユーザ名を含むパス等）は伏せる。
トラブルへの対処は docs/usage.md、既知の制限は docs/design.md（Windows の制約は docs/windows.md）にある。
-->

**環境 / Environment**
<!--
OS・セッション: Ubuntu 24.04・Wayland、Windows 11 等
IME とバージョン: fcitx5 5.1.7＋Mozc 2.28、Microsoft IME 等
キーボード: 配列と接続（JIS 配列・Bluetooth 等）
使用方法: 常駐サービス（Linux。`install.sh` でインストールし、ログイン時に自動で起動する）、または kanata 単体（常駐サービスを使わずに、生成済みの設定ファイルを kanata に指定して起動する。Windows での試用を含む）
kanata-nicola のコミット: リポジトリの直下で `git rev-parse --short HEAD` を実行した出力
kanata のバージョン: `NICOLA_KANATA_BIN` を指定した場合と、kanata 単体で使う場合だけ（使用している kanata の `--version` の出力）
-->
- OS・セッション:
- IME とバージョン:
- キーボード:
- 使用方法:
- kanata-nicola のコミット:
- kanata のバージョン:

**設定 / Configuration**
<!--
常駐サービスの場合: `~/.config/kanata-nicola/config.env` の `#` で始まらない行。利用者の配列差分ファイルや追加の kanata の設定ファイルを指定している場合は、その内容も
kanata 単体で使う場合: 指定した配列プロファイルと、編集した箇所・再生成のオプション
-->

**アプリケーション / Application**
<!-- 入力したアプリケーションと、ほかのアプリケーションでも発生するか -->

**操作 / What you typed**
<!-- 入力したキー（押した順と、押したまま次のキーを押したか。例: 左の親指キー＋W）と、意図した文字 -->

**結果 / What happened**
<!-- 入力された文字、またはキーの動作 -->

**頻度 / How often**
<!-- 1 回だけ / 不定期 / 毎回 -->

**ログ / Logs**
<!-- 常駐サービスの場合は、症状が発生した前後のログ（コマンドは docs/usage.md）。kanata 単体で使う場合は、kanata のコンソールの出力 -->

**補足 / Notes**
