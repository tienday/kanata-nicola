# 設定リファレンス

この文書は、常駐サービス（Linux）が読み込む設定ファイル `config.env` の書式と設定項目を説明する。

kanata 単体で使う場合（Windows での試用を含む）は、この設定ファイルは使用しない（[kanata の設定ファイルの手引き](kanata-kbd.md)、[Windows での利用 §4](windows.md#4-設定の変更)）。

---

## 1. 設定ファイル

| 項目 | 内容 |
|---|---|
| 場所 | `~/.config/kanata-nicola/config.env`（`XDG_CONFIG_HOME` を設定している場合は、その下の `kanata-nicola/config.env`）。ファイルが存在しない場合は、すべての設定が既定値（[§2](#2-設定項目)）になる |
| ひな形 | `platform/linux/config.env.example`（コピーし、使用する行の `#` を削除して編集する） |
| 書式 | 1 行に 1 つ、変数の代入（例: `NICOLA_IME_SYNC=1`）を記述する。ファイルはシェル（bash）のスクリプトとして読み込まれるため、代入以外は記述しない。複数の値は `:` で区切る |
| キーコード名 | `NICOLA_THUMB_KEYS`・`NICOLA_KEYS_AS_CTRL` では、キーボードのキーを Linux のキーコード名（`KEY_CAPSLOCK` 等）で指定する |
| ファイルのパス | `NICOLA_LAYOUT_OVERLAYS`・`NICOLA_EXTRA_KBD` のファイルは、`/` で始まる場合は絶対パス、それ以外は設定ファイルのディレクトリ（`~/.config/kanata-nicola/`）からの相対パスとして扱う |
| 反映 | 常駐サービスは、起動のたびに設定を読み込む。`config.env` と、そこで指定したファイル（配列差分ファイル、追加の kanata の設定ファイル）を更新した場合は、`systemctl --user restart kanata-nicola` で反映する（`NICOLA_KANATA_BIN` の指定を削除した場合を除く。[§2](#2-設定項目)） |
| 誤りがある場合 | 起動時にエラーとなる（対処は[導入と利用 §4](usage.md#4-トラブルへの対処)）。[§2](#2-設定項目) の表に無い変数は無視する（名前が `NICOLA_` で始まるものは、警告を表示する） |

## 2. 設定項目

| 設定 | 用途 | 既定 | 例 |
|---|---|---|---|
| `NICOLA_IME_SYNC` | `1` で、NICOLA を IME の実際の状態に追従させる（[導入と利用 §3.1](usage.md#31-nicola-入力の-onoff)）。fcitx5＋Mozc に対応。半角/全角は、そのまま出力する。kanata の TCP ポート（`NICOLA_KANATA_PORT`）を開く | `0`（NICOLA 切替キーの方式） | `1`（fcitx5 では推奨） |
| `NICOLA_KEYS_AS_CTRL` | 左右の Ctrl に加えて、Ctrl として扱うキー。デスクトップのキー配列の設定（xkb）で Ctrl に変更したキー（CapsLock 等。[§3.1](#31-設定値の調べ方)）を指定する。指定しない場合は、NICOLA が ON の間、そのキーとの組み合わせがショートカットとして機能しない。Alt・Super と Ctrl の入れ替え（`ctrl:swap_lalt_lctl` 等）では不要 | 空（左右の Ctrl だけ） | `KEY_CAPSLOCK` |
| `NICOLA_THUMB_KEYS` | 左右の親指キー（`左:右`）。主な候補は `KEY_SPACE`・`KEY_MUHENKAN`（無変換）・`KEY_HENKAN`（変換）・`KEY_KATAKANAHIRAGANA`（カタカナ/ひらがな）。修飾キーは指定できない。NICOLA が ON の間、親指キーを単独で押した場合は、離したときにキー本来の入力を送出する（リピートしない。Space の場合は、空白や変換も離したときに入力される） | 空（左＝無変換、右＝変換。`KEY_MUHENKAN:KEY_HENKAN` と同じ） | `KEY_SPACE:KEY_HENKAN`（左を Space に変更） |
| `NICOLA_LAYOUT_OVERLAYS` | 配列差分ファイル。指定した順に重ねる。同梱のものは名前（`.toml` を付けない）で指定する（[配列差分ファイル](layout-overlays.md)） | 空（NICOLA 標準） | `practical:lag-tolerant:user-layout.toml` |
| `NICOLA_EXTRA_KBD` | 追加の kanata の設定ファイル（キーの再割り当て等。[§3.2](#32-追加の-kanata-の設定ファイル)） | 空（なし） | `user-extra.kbd` |
| `NICOLA_INCLUDE_DEVICES` | 対象にするキーボードの名前（[§3.1](#31-設定値の調べ方)） | 空（後から接続したものも含め、すべてのキーボード） | `AT Translated Set 2 keyboard` |
| `NICOLA_EXCLUDE_DEVICES` | 除外するキーボードの名前（NICOLA に使用しない、キーのある入力デバイス等） | 空 | `Example Extra Buttons` |
| `NICOLA_WAIT_DEVICE_MS` | キーボードの接続時に、kanata が入力デバイスを探し直す前に待つ時間（ms）。待つ間は、すべてのキーボードの入力が遅れる | `0`（`NICOLA_KANATA_BIN` を指定した場合は kanata の既定。その kanata では、待たない場合は、接続直後のキーボードを取得できないことがある） | — |
| `NICOLA_KANATA_PORT` | IME 同期で使用する kanata の TCP ポート（1〜65535）。同じ PC のプロセスだけが、利用者を問わず接続できる（127.0.0.1。[設計書 §10.5](design.md#105-tcp-ポート)） | `23800` | — |
| `NICOLA_KANATA_BIN` | 使用する kanata の実行ファイル。修正を適用した kanata 以外では、キーボードの再接続の修正が不足する（[設計書 §10.1](design.md#101-使用するバージョンと修正)）。また IME 同期では、TCP ポートに接続できるプロセス（同じ PC のほかの利用者のものを含む）が、別の設定を kanata に読み込ませて、キー入力を読み取れる。指定を削除した場合は、修正を適用した kanata をインストールするため、`platform/linux/install.sh` を実行する | 空（常駐サービスのインストール時にビルドする、修正を適用した kanata） | `/usr/local/bin/kanata` |

`config.env` では変更できないもの:

- 判定: 親指先行方式（連続シフトを含む）
- 送出: ローマ字方式（デスクトップのキー配列が日本語〔xkb の `jp`〕であることが前提）
- NICOLA 切替キー（IME 同期を使用しない場合）: 半角/全角（kanata 単体で使う場合は、kanata 設定生成器〔`nicola-kbdgen`〕での再生成により変更可。[kanata の設定ファイルの手引き §3](kanata-kbd.md#3-設定ファイルの再生成)）

## 3. 設定項目の詳細

### 3.1 設定値の調べ方

| 調べること | 方法 | 設定 |
|---|---|---|
| CapsLock を Ctrl にしているか（GNOME） | `gsettings get org.gnome.desktop.input-sources xkb-options` の結果に、`ctrl:nocaps`・`ctrl:swapcaps`（`ctrl:swapcaps_hyper` 等を含む）・`caps:ctrl_modifier` のいずれかが含まれるか | `NICOLA_KEYS_AS_CTRL=KEY_CAPSLOCK` |
| キーボードの名前 | kanata が取得したキーボード: kanata のログの `registering` の行（常駐サービスでは `journalctl --user -u kanata-nicola \| grep registering`）。すべての入力デバイス: `grep Name /proc/bus/input/devices` | `NICOLA_INCLUDE_DEVICES`、`NICOLA_EXCLUDE_DEVICES` |

### 3.2 追加の kanata の設定ファイル

kanata の設定を追加する場合（キーの再割り当て等）は、kanata の書式のファイルを `NICOLA_EXTRA_KBD` に指定する。このファイルは、kanata-nicola が生成する kanata の設定ファイルの末尾で、`include` によって読み込まれる。

例: 文字キー（配列で文字を割り当てたキー）以外のキー（ここでは PrintScreen）に、別のキー（ここでは Menu キー）を割り当てる。

```sh
NICOLA_EXTRA_KBD=user-extra.kbd     # config.env に追加する
```

```lisp
;; ~/.config/kanata-nicola/user-extra.kbd
(defoverrides
  (prnt) (menu)     ;; キーの組み合わせも指定できる（例: (lsft f10)）
)
```

| 項目 | 内容 |
|---|---|
| 用途 | 文字キー以外のキーに、別のキーやキーの組み合わせを割り当てる |
| 記述方法 | kanata の `defoverrides`（または `defoverridesv2`）で、キーやその組み合わせを、別のキーや組み合わせに置き換える（kanata の文書の [Global overrides](https://github.com/jtroo/kanata/blob/main/docs/config.adoc#global-overrides)）。kanata の出力を置き換えるため、NICOLA の ON/OFF に関係なく有効 |
| 使用できない定義 | `defcfg`・`defsrc`・レイヤ（生成する設定ファイルに定義されているため、重複のエラーとなる）、`include`（入れ子にできない） |
| 注意 | 英字・記号のキーと親指キーは置き換えない。kanata-nicola は、これらのキーで IME にローマ字を送出するため、置き換えると入力される文字が変わる |
| kanata 単体で使う場合 | `NICOLA_EXTRA_KBD` は使用できない。同じ定義を、環境固有設定（キーボードの選択などを記述する kanata の設定ファイル。[kanata の設定ファイルの手引き §2.1](kanata-kbd.md#21-環境固有設定user-customkbd)）に追加する |
