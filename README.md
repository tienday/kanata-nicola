# kanata-nicola

Thumb-shift (NICOLA) Japanese input with [kanata](https://github.com/jtroo/kanata). The documentation is in Japanese.

- Generates kanata configurations (`.kbd`) from the NICOLA layout and small overlay files. Prebuilt ones are in `kbd/` and run on the official kanata.
- Thumb-prefix method: hold a thumb key (Muhenkan or Henkan by default), then press a character key. The character is sent at once, as romaji keys to the IME (Mozc).
- Linux (Ubuntu 24.04 / 26.04): an installer with systemd user services, and `nicola-imesync`, which turns NICOLA on and off with the state of fcitx5 + Mozc.

A chord needs the thumb key first, unlike the NICOLA standard, whose judgment a kanata configuration cannot express; see [docs/design.md §3](docs/design.md#3-nicola-規格との違い).

---

キー割り当てツール [kanata](https://github.com/jtroo/kanata) で、親指シフト（NICOLA）の日本語入力を実現するソフトウェア。

## 特徴

- **配列定義から kanata の設定を生成する:** NICOLA J型の標準配列に、変更する面だけを記述した配列差分ファイルを重ねて調整できる。
- **親指先行方式:** 親指キーを押したまま文字キーを押すとシフト面の文字になり、判定時間を待たずに入力される。
- **常駐サービスとして動作する:** Linux ではログイン時に自動で起動し、kanata を手動で操作する必要はない。生成済みの設定ファイルを kanata 単体で使うこともできる（任意。[kanata の設定ファイルの手引き](docs/kanata-kbd.md)）。
- **IME の状態に追従する:** IME（fcitx5＋Mozc）がひらがな・カタカナのときだけ NICOLA を ON にする（ウィンドウの切り替えやマウスでの切り替えにも追従する）。
- **kanata の設定を追加できる:** キーの再割り当てなど、kanata の書式で記述した設定ファイルを、生成する設定に取り込める（[設定リファレンス §3.2](docs/settings.md#32-追加の-kanata-の設定ファイル)）。

## 入力の仕組み

キーボードからの入力は kanata が受け取り、NICOLA の配列で決まる文字を、ローマ字のキー入力として IME に送出する（例: 「か」なら `k` `a`）。IME はローマ字入力の設定のまま使用する。

親指キーは、既定では左が無変換、右が変換である（設定により変更可）。

| 操作 | 結果 |
|---|---|
| 文字キーだけを押す | 単独面の文字（例: W で「か」） |
| 親指キーを押したまま文字キーを押す | シフト面の文字（例: W で、左の親指キーなら「え」、右なら「が」）。親指キーを押している間は、続けて押した文字もシフト面になる |
| 文字キーを押してから親指キーを押す | 同時打鍵にならない |
| 親指キーだけを押して離す | そのキー本来の機能（無変換・変換など） |

### NICOLA 規格との違い

kanata-nicola の判定は、NICOLA 規格と次の点で異なる（一覧と理由の詳細は[設計書 §3](docs/design.md#3-nicola-規格との違い)）。

| 機能 | NICOLA 規格 | kanata-nicola |
|---|---|---|
| 同時打鍵になる押下順 | 親指キー・文字キーのどちらが先でもよい | 親指キーを先に押したときだけ |
| 親指キー 1 回の押下でシフト面になる文字 | 1 文字 | 押している間のすべての文字（連続シフト。NICOLA 規格には無い拡張） |
| 文字が入力される時点 | 判定が付いてから（判定時間 50〜200ms） | 押した瞬間（判定時間なし） |
| 3キー判定・薄い重なり判定 | あり（必須要件） | なし（親指キーを先に押す形だけを扱うため不要） |

**この方式にしている理由**

- 規格の判定は、kanata の設定では書き表せない（1.12.0 まで）。文字キーを先に押す同時打鍵（文字先行）を含めて判定する方式も検証したが、誤判定や入力の取りこぼしが発生した。
- 判定を待たずに文字が入力され、押下順による誤判定も起きない。
- 代償として、親指キーを離すのが遅れると、次の文字がシフト面になる（離し遅れ）。
  - 対策の例として、配列差分ファイル `lag-tolerant` を同梱している（[配列差分ファイル §3.2](docs/layout-overlays.md#32-lag-tolerant離し遅れの対策)）。
- 規格に沿った判定は kanata-nicola の範囲外とし、NICOLA 判定エンジン（別途開発、現時点では未公開。[設計書 §1.2](docs/design.md#12-用語)）で扱う。

## 動作環境

| OS | IME | サポート | 内容 |
|---|---|---|---|
| Ubuntu 24.04・26.04 LTS | fcitx5＋Mozc | 対象（推奨） | IME の ON/OFF に合わせて、NICOLA も自動で ON/OFF する |
| Ubuntu 24.04・26.04 LTS | ibus＋Mozc | 対象 | IME の状態には追従しない。半角/全角キーで、IME と NICOLA を一緒に ON/OFF する |
| Windows 11 | Microsoft IME | 正式サポート前（試用は可能） | NICOLA の入力は、kanata の Windows 版と生成済みの設定で動作を確認している。導入のスクリプト・常駐・IME の状態への追従が無いため、起動と ON/OFF は手動で行う（[Windows での利用](docs/windows.md)） |

- キーボードは JIS 配列（内蔵・外付け）。既定の親指キー（無変換・変換）は、JIS 配列のキーである。
- Ubuntu は、X11・Wayland のどちらのセッションでも使える。
- Linux では、キーの読み取りと送出に evdev・uinput を、常駐に systemd のユーザサービスを使う。動作の条件と既知の制限の詳細は、[設計書 §4.2](docs/design.md#42-動作の前提)・[§12](docs/design.md#12-既知の制限と未解決事項) にある。

## クイックスタート（Linux）

標準の利用形態（常駐サービス）で導入する手順を示す。手順の詳細と注意は[導入と利用](docs/usage.md)にある。

**準備:** IME は fcitx5＋Mozc を推奨する（IME の状態に NICOLA が追従する）。Ubuntu の標準の IME は ibus なので、fcitx5 をインストールし、ログイン時に起動するよう設定する（[導入と利用 §2.2](docs/usage.md#22-ime-の準備)）。

**導入**

1. ビルドに必要なものをインストールする。Rust（1.85 以降）は [rustup](https://rustup.rs) の手順で、パッケージ（Ubuntu）は次のコマンドでインストールする。

   ```sh
   sudo apt install build-essential pkg-config libdbus-1-dev curl patch git
   ```

2. リポジトリを取得する。以降のコマンドは、リポジトリのディレクトリで実行する。

   ```sh
   git clone https://github.com/tienday/kanata-nicola.git && cd kanata-nicola
   ```

3. 権限を設定する（初回のみ。設定した後は、ログインし直す）。キーボードの読み取りとキーの送出のため、利用者を input・uinput グループに加える（[kanata の Linux 向けの案内](https://github.com/jtroo/kanata/blob/main/docs/setup-linux.md)と同じ方法）。このグループの権限は、その利用者が実行するほかのプログラムにも及ぶ（[導入と利用 §2.1](docs/usage.md#21-権限の設定)）。

   ```sh
   platform/linux/permissions.sh
   ```

4. 設定ファイルを作成し、必要に応じて設定を変更する（fcitx5 の場合は、IME 同期の設定 `NICOLA_IME_SYNC=1` を有効にする）。

   ```sh
   mkdir -p ~/.config/kanata-nicola
   cp platform/linux/config.env.example ~/.config/kanata-nicola/config.env
   ```

5. ビルドし、常駐サービスとして登録して起動する。

   ```sh
   platform/linux/install.sh --start
   ```

**導入後の操作**

| 操作 | 方法 |
|---|---|
| 起動する・設定の変更を反映する | ログイン時に自動で起動する。手動で起動・反映するときは `systemctl --user restart kanata-nicola` |
| 停止する | `systemctl --user stop kanata-nicola`（次回のログインで再び起動する） |
| 非常停止（キーが意図しない動作をして、操作できないとき） | **左Ctrl+Space+Esc**（再開は `systemctl --user restart kanata-nicola`） |
| 動作を確認する | [導入と利用 §2.5](docs/usage.md#25-動作の確認) の操作を試す |
| 更新する | `git pull` の後に `platform/linux/install.sh` |
| アンインストールする | `platform/linux/uninstall.sh`（設定と権限の設定は残る。[導入と利用 §5.2](docs/usage.md#52-アンインストール)） |

## ドキュメント

**利用者向け**

| ドキュメント | 内容 |
|---|---|
| [導入と利用](docs/usage.md) | Linux での導入、日常の操作、トラブルへの対処、更新とアンインストール |
| [設定リファレンス](docs/settings.md) | 設定項目の一覧と、設定値の調べ方 |
| [配列差分ファイル](docs/layout-overlays.md) | 標準配列の一部を変更する配列差分ファイルの仕組み・利用方法・書式、同梱の配列差分ファイルの内容 |
| [kanata の設定ファイルの手引き](docs/kanata-kbd.md) | kanata 単体で使う方法と、生成済みの設定ファイル（`.kbd`）の構成・編集方法 |
| [Windows での利用](docs/windows.md) | Windows で試用する方法と、Linux 版との違い |

**開発者向け**

| ドキュメント | 内容 |
|---|---|
| [設計書](docs/design.md) | 方式と構成、その設計判断と制約 |
| [実装仕様書](docs/implementation.md) | ソースの構成、各コンポーネントの仕様（コマンド、ファイルの配置と形式）、テストの方法 |
| [kanata の設定ファイルの設計](docs/kanata-kbd-design.md) | 生成する設定ファイルで NICOLA の判定・送出・切り替えを実現する定義の組み方と、命名規則 |

## 不具合の報告・要望

不具合の報告と機能の要望は、GitHub の Issue で受け付ける。

## 開発

本ソフトウェアは、[Claude Code](https://claude.com/product/claude-code) の支援を受けて開発している。

## 類似のソフトウェア

- [oyainput](https://github.com/inwskatsube/oyainput): Linux で親指シフト入力をするソフトウェア。

## 出典

- [kanata](https://github.com/jtroo/kanata)（LGPL-3.0-only）: 生成した設定ファイルでキー入力を処理するキー割り当てツール。
  - 本リポジトリは、kanata に適用するパッチ（上流のコミット 1 件を含む。[修正を適用した kanata](platform/linux/kanata/README.md)）のほかは、kanata のソースコードおよび実行ファイルを含まない。
- NICOLA 日本語入力コンソーシアムの [NICOLA 配列規格書](http://nicola.sunicom.co.jp/spec/kikaku.htm)・[JIS 提案](http://nicola.sunicom.co.jp/spec/jisdraft.htm)・[実装情報](http://nicola.sunicom.co.jp/info2.html)。配列定義は、これらに基づき独自の形式で記述した。
- その他、参考資料の一覧と用途: [設計書 §13](docs/design.md#13-参考資料)

## ライセンス

次のいずれかのライセンスを選択して利用できる（MIT OR Apache-2.0）。

- [MIT License](LICENSE-MIT)
- [Apache License 2.0](LICENSE-APACHE)

ただし、kanata に適用するパッチ（`platform/linux/kanata/patches/`）は、kanata と同じ [LGPL-3.0-only](platform/linux/kanata/patches/COPYING.LESSER) とする。

本リポジトリへの貢献は、別の条件を明示しない限り、上記と同じライセンスで提供されたものとする。
