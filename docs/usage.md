# 導入と利用

この文書は、kanata-nicola を Linux（Ubuntu）に導入し、常駐サービスとして使用する手順を説明する。

常駐サービスを使用しない方法は[kanata の設定ファイルの手引き](kanata-kbd.md)、Windows で試用する方法は[Windows での利用](windows.md)にある。

---

## 1. 動作環境

| 項目 | 条件 |
|---|---|
| OS | Ubuntu 24.04・26.04 LTS（X11・Wayland のどちらのセッションでも使用できる）。キーの読み取りと送出に、Linux の evdev・uinput を使用する |
| キーボード | JIS 配列。既定の親指キーは左＝無変換、右＝変換（設定により変更可。[設定リファレンス §2](settings.md#2-設定項目) の `NICOLA_THUMB_KEYS`） |
| デスクトップのキー配列 | 日本語（xkb の `jp`） |
| IME | fcitx5＋Mozc（推奨）、または ibus＋Mozc。Mozc は**ローマ字入力**（Mozc の既定）のまま使用する。IME 同期（[§3.1](#31-nicola-入力の-onoff)）は fcitx5 だけに対応し、ibus では NICOLA 切替キーの方式で使用する（準備は [§2.2](#22-ime-の準備)） |
| ビルド | Rust 1.85 以降と、C のリンカ、IME 同期の常駐ツール nicola-imesync が使用する D-Bus の開発用ファイル、kanata のソースの取得と修正に使用するツール（インストールは [§2](#2-導入)） |
| kanata | 常駐サービスのインストール時に、キーボードの再接続などの修正を適用した kanata をビルドして使用する（[設計書 §10.1](design.md#101-使用するバージョンと修正)） |
| 権限 | キーボードを読み取り、仮想キーボードを作成する権限（input・uinput グループ。[§2.1](#21-権限の設定)） |
| 常駐 | systemd のユーザサービス |

## 2. 導入

導入は、次の順に行う。

1. ビルドに必要なものをインストールする。Rust は [rustup](https://rustup.rs) の手順で、パッケージは次のコマンドでインストールする。

   ```sh
   sudo apt install build-essential pkg-config libdbus-1-dev curl patch git
   ```

2. kanata-nicola のリポジトリを取得する。以降のコマンドは、リポジトリのディレクトリで実行する。

   ```sh
   git clone https://github.com/tienday/kanata-nicola.git && cd kanata-nicola
   ```

3. 権限を設定する（[§2.1](#21-権限の設定)）。
4. IME を準備する（[§2.2](#22-ime-の準備)）。
5. ログインし直す。手順 3・4 の設定が有効になる。
6. 設定ファイルを作成する（[§2.3](#23-設定ファイルの作成)）。
7. 常駐サービスをインストールする（[§2.4](#24-常駐サービスのインストール)）。
8. 動作を確認する（[§2.5](#25-動作の確認)）。

### 2.1 権限の設定

kanata は、キーボードのデバイス（`/dev/input/event*`）を読み取り、`/dev/uinput` で仮想キーボードを作成してキーを送出する。どちらも一般の利用者の権限では実行できないため、利用者を input・uinput グループに追加する。Linux のキー割り当てツールで一般的な方法で、[kanata の Linux 向けの案内](https://github.com/jtroo/kanata/blob/main/docs/setup-linux.md)と同じである。初回だけ行う。

**権限の及ぶ範囲:** この 2 つのグループの権限は、kanata だけでなく、その利用者が実行するほかのプログラムにも及ぶ。それらのプログラムも、すべてのキー入力（パスワードを含む）を読み取り、キー入力を送出できるようになる。kanata は root ではなく利用者の権限で動作し、sudo を使用するのはこの権限の設定だけである。元に戻す方法は [§5.2](#52-アンインストール) にある。

```sh
platform/linux/permissions.sh
```

`permissions.sh` は、不足している権限を調べ、次のうち不足しているものを設定するコマンド（sudo を使用する）を表示して、確認を求めてから実行する。

- 利用者を input グループに追加する（キーボードを読み取るため）
- uinput グループを作成し、`/dev/uinput` をそのグループで読み書きできるようにする udev の規則を `/etc/udev/rules.d/99-input.rules` に記述する（ファイルが存在する場合は変更しない）
- 利用者を uinput グループに追加する

グループへの追加は、ログインし直す（または PC を再起動する）と有効になる。

### 2.2 IME の準備

**fcitx5（推奨。IME 同期を使用できる）:** Ubuntu の標準の IME は ibus なので、fcitx5 をインストールし、ログイン時に起動するよう設定する。

```sh
sudo apt install fcitx5 fcitx5-mozc fcitx5-config-qt
im-config -n fcitx5
```

Wayland のセッションでは、`im-config` だけでは fcitx5 が起動しない。[fcitx5 の案内（Using Fcitx 5 on Wayland）](https://fcitx-im.org/wiki/Using_Fcitx_5_on_Wayland)に従い、自動起動と環境変数も設定する。

```sh
mkdir -p ~/.config/autostart ~/.config/environment.d
cp /usr/share/applications/org.fcitx.Fcitx5.desktop ~/.config/autostart/
printf 'GTK_IM_MODULE=fcitx\nQT_IM_MODULE=fcitx\nXMODIFIERS=@im=fcitx\n' > ~/.config/environment.d/fcitx5.conf
```

ログインし直した後（[§2](#2-導入) の手順 5）に、次の設定をする。

- fcitx5 の設定（`fcitx5-configtool`）で、入力メソッドに「キーボード - 日本語」と「Mozc」を追加する。
- GNOME では、次の拡張のどちらかを有効にする（Ubuntu の標準のデスクトップでは、トレイを表示する拡張が有効）。IME 同期は、Mozc の入力モードを、fcitx5 がトレイか入力パネルに表示する情報から読み取る。
  - トレイを表示する拡張（AppIndicator の拡張）
  - 入力パネルの拡張（Input Method Panel。入力中の文字や変換の候補を表示する）

**ibus（Ubuntu の標準）:** NICOLA 切替キーの方式（[§3.1](#31-nicola-入力の-onoff)）で使用する。GNOME の入力ソースに「日本語」と「日本語 (Mozc)」の両方を追加し、「日本語 (Mozc)」を選択して使用する（「日本語 (Mozc)」だけでは、半角/全角で IME が切り替わらず、NICOLA だけが切り替わる）。

### 2.3 設定ファイルの作成

設定は `~/.config/kanata-nicola/config.env` に記述する。ひな形をコピーして編集する（ファイルが存在しない場合は、既定値で動作する）。

```sh
mkdir -p ~/.config/kanata-nicola
cp platform/linux/config.env.example ~/.config/kanata-nicola/config.env
```

多くの場合、次の項目を確認すれば十分である。設定の一覧と書式は[設定リファレンス](settings.md)にある。

| 項目 | 設定 |
|---|---|
| IME の状態に NICOLA を追従させる（推奨。fcitx5＋Mozc の場合） | `NICOLA_IME_SYNC=1`（[§3.1](#31-nicola-入力の-onoff)） |
| CapsLock を Ctrl にしている（調べ方は[設定リファレンス §3.1](settings.md#31-設定値の調べ方)） | `NICOLA_KEYS_AS_CTRL=KEY_CAPSLOCK` |
| 配列を調整する（任意） | 例: `NICOLA_LAYOUT_OVERLAYS=practical:lag-tolerant`（[配列差分ファイル](layout-overlays.md)） |

### 2.4 常駐サービスのインストール

ほかのキー割り当てツールや親指シフトのエミュレーターが動作している場合は、先に停止する（同時に動作させると、キーボードの取り合いが起きる）。

次のコマンドで、プログラムをビルドし、常駐サービスとして登録して起動する。

```sh
platform/linux/install.sh --start
```

- 初回は kanata もビルドするため、数分程度かかる。
- 書き込むのは、インストール先（`~/.local/share/kanata-nicola/`）、ユニットファイル（`~/.config/systemd/user/`）、リポジトリの `target/`（ビルドの成果物）だけである。

### 2.5 動作の確認

インストールした後に、次の操作で正しく動作するかを確認する。

| 操作 | 期待する結果 |
|---|---|
| IME を ON にする（IME 同期のときは、Mozc をひらがなにする） | NICOLA が ON になる |
| 文字キーだけを押す（例: W） | 「か」が、押した瞬間に入力される |
| 親指キーを押したまま文字キーを押す（左の親指キー＋W、右の親指キー＋W。既定の親指キーは無変換・変換） | 「え」「が」 |
| 親指キーだけを押して離す | そのキー本来の機能（無変換・変換の場合は、Mozc の変換など） |
| NICOLA が ON のまま Ctrl+C・Ctrl+V・Shift+英字 | 通常のショートカット・英大文字として機能する |
| IME を OFF にする | NICOLA も OFF になり、英字がそのまま入力される |
| 外付けキーボードを抜き差しする | 数秒で、再び NICOLA で入力できる |

## 3. 日常の操作

### 3.1 NICOLA 入力の ON/OFF

NICOLA は、IME で日本語を入力するときだけ ON にする。切り替え方は、方式（[§2.3](#23-設定ファイルの作成) の `NICOLA_IME_SYNC`）で決まる。

| 方式 | 切り替え方 |
|---|---|
| IME 同期（推奨。fcitx5＋Mozc） | IME を切り替えるだけでよい。Mozc がひらがな・カタカナのときに ON、直接入力・英数のときに OFF になる。IME を切り替えるキー、マウス、ウィンドウの切り替え、トレイや入力パネルのメニューのどれで切り替えても追従する |
| NICOLA 切替キーの方式（既定。ibus はこの方式） | 半角/全角で、IME と NICOLA を一緒に切り替える。NICOLA は OFF で始まるので、IME も OFF にしておく。IME の実際の状態を参照しないため、ずれることがある。ずれた場合は、IME の側（トレイのメニュー等）で、IME の ON/OFF を NICOLA に合わせる |

NICOLA が ON の間も、Ctrl・Alt・Super・Shift を押している間は、キーをそのまま出力する（Ctrl+C 等のショートカットや Shift+英字が機能する）。CapsLock を Ctrl にしている場合は、[§2.3](#23-設定ファイルの作成) の設定が必要である。

### 3.2 起動・停止

常駐サービスは、デスクトップへのログイン時に起動し、ログアウト時に停止する。操作は次のコマンドで行う。

```sh
systemctl --user restart kanata-nicola     # 起動する・設定の変更を反映する（動作中の場合は再起動する）
systemctl --user stop kanata-nicola        # 停止する（次回のログイン時に再び起動する）
systemctl --user disable kanata-nicola     # ログイン時に起動しないようにする（動作中の場合は動作を続ける。元に戻すには enable）
systemctl --user status kanata-nicola kanata-nicola-imesync     # 状態を表示する
```

- 起動の直後の数秒間は、キーを押さない（kanata がキーボードを取得するまでの間）。
- `disable` の後に `install.sh` を実行し直すと、ログイン時に起動する設定に戻る。
- nicola-imesync は、kanata-nicola と一緒に起動・停止する（IME 同期を使用しない設定では起動しない）。
- SSH など、デスクトップの無いログインでは起動しない。

**非常停止:** キーが意図しない動作をして操作できないときは、**左Ctrl+Space+Esc** を押す（kanata の標準）。kanata が停止し、キーボードを通常どおり使用できるようになる。非常停止で停止した kanata は、自動では再起動しない（nicola-imesync も停止する）。再開するには `systemctl --user restart kanata-nicola` を実行する。

**同じ PC を複数の利用者で使用する場合:** 次の利用者がログインする前に、ログアウトする。ログアウトせずに別の利用者に切り替える使用方法には対応しない。切り替えると、前の利用者の kanata がキーボードを取得したまま残り、次の利用者の入力（ログイン画面のパスワードを含む）がその kanata を経由する（対処は [§4](#4-トラブルへの対処)）。

### 3.3 設定の変更

`~/.config/kanata-nicola/config.env` を更新し、常駐サービスを再起動して反映する。設定に誤りがある場合は、起動時にエラーとなる（対処は [§4](#4-トラブルへの対処)）。

```sh
systemctl --user restart kanata-nicola
```

設定の一覧（キーの再割り当てを含む）は[設定リファレンス](settings.md)、配列の調整は[配列差分ファイル](layout-overlays.md)にある。

## 4. トラブルへの対処

常駐サービスのログは、systemd のユーザジャーナルに記録される。原因を調べる場合は、次のコマンドでログを表示する（ログに記録される内容は[実装仕様書 §5.6](implementation.md#56-ログ)）。

```sh
journalctl --user -u kanata-nicola -u kanata-nicola-imesync -f       # 新しいログを続けて表示する
journalctl --user -u kanata-nicola --since today | grep -E 'removing|registering|found device'  # キーボードの切断と再取得
journalctl --user -u kanata-nicola-imesync --since today             # IME の状態の変化
```

| 症状 | 対処 |
|---|---|
| スクリプトを実行すると `Permission denied` と表示される | スクリプトの実行権限が無い（リポジトリを `git clone` 以外の方法でコピーした場合など）。リポジトリの直下で `chmod +x platform/linux/*.sh platform/linux/kanata/build.sh` を実行する |
| NICOLA で入力できない（キーボードは NICOLA なしで使用できる。設定を変更した後など） | 常駐サービスが起動していない。`systemctl --user status kanata-nicola` に理由（設定の値の誤り、ほかに動作中の kanata など）が表示されるので、原因を取り除いてから `systemctl --user restart kanata-nicola` を実行する |
| 起動の失敗が続き、停止した | 失敗が続くと、systemd が再起動を中止する。原因を取り除いてから、`systemctl --user reset-failed kanata-nicola` と `systemctl --user restart kanata-nicola` を実行する |
| NICOLA が ON のときだけ、Ctrl のショートカットが異なる動作になる（CapsLock+A で全選択にならず、Ctrl+U など別のキーとして機能する） | CapsLock を Ctrl にしている環境で起きる。`config.env` に `NICOLA_KEYS_AS_CTRL=KEY_CAPSLOCK` を記述する（[§2.3](#23-設定ファイルの作成)。GNOME では、記述しない場合は警告が表示される） |
| IME が OFF なのに NICOLA が ON（ローマ字がそのまま入力される） | NICOLA 切替キーの方式のずれ。IME の側で合わせる（[§3.1](#31-nicola-入力の-onoff)）。fcitx5 の場合は、IME 同期（`NICOLA_IME_SYNC=1`）にすると起きない |
| IME 同期で、NICOLA が IME に追従しない（Mozc が直接入力・英数のときも ON になる等） | GNOME では、トレイを表示する拡張か入力パネルの拡張が有効かを確認する（[§2.2](#22-ime-の準備)）。IME の状態を読み取れているかは、`cargo run -q --release -p nicola-imesync -- --dry-run` で確認できる。Mozc のモードを変更するたびに `fcitx5=2 im=mozc mozc=hiragana -> nicola` のような行が表示され、ひらがな・カタカナで `-> nicola`、それ以外で `-> base` の場合は、正しく読み取れている |
| キーが押されたままになった | そのキーを一度押して離す。解消しない場合は、非常停止する（[§3.2](#32-起動停止)） |
| キーボードを接続したときに、どのキーボードでも入力できなくなった（非常停止も機能しない） | kanata は、接続したキーボードのキーがすべて離されるまで、ほかのキーボードの入力も処理しない。接続したキーボードで押されているキーを離す。解消しない場合は、そのキーボードを取り外す |
| 外付けキーボードを接続し直した後は、NICOLA で入力できない | ログでキーボードの再取得を確認する（上の 2 つ目のコマンド）。`NICOLA_KANATA_BIN` で別の kanata を指定している場合は、その指定を削除して `platform/linux/install.sh` を実行し、修正を適用した kanata を使用する |
| ログイン画面で、パスワードを入力してもログインできない（ログアウトせずに利用者を切り替えた後） | 前の利用者の kanata が NICOLA のまま残り、パスワードを変換している。非常停止（左Ctrl+Space+Esc）でその kanata を停止してから入力する（[§3.2](#32-起動停止)） |

## 5. 更新とアンインストール

### 5.1 更新

次のコマンドで、更新を取得してからインストールし直す（動作中の常駐サービスは再起動される）。

```sh
git pull
platform/linux/install.sh
```

### 5.2 アンインストール

次のコマンドで、サービスの登録と `~/.local/share/kanata-nicola/` を削除する（削除の前に確認を求める。`-y` を指定すると確認を省略する）。設定（`~/.config/kanata-nicola/`）と権限の設定（[§2.1](#21-権限の設定)）は削除しない。

```sh
platform/linux/uninstall.sh
```

- 設定も削除する場合は、`rm -r ~/.config/kanata-nicola` を実行する。
- リポジトリのディレクトリには、ビルドの成果物（`target/`。1 GB 程度）が含まれる。不要な場合は、ディレクトリごと削除する。

**権限の設定も元に戻す場合:** input・uinput グループを使用するほかのツール（キー割り当てツール、kanata 単体で使う場合等）が無いことを確認してから、次の操作を行う。

1. 利用者を input・uinput グループから削除する: `sudo gpasswd -d $USER input` と `sudo gpasswd -d $USER uinput`（ログインし直すと有効になる）。
2. uinput グループを削除する: `sudo groupdel uinput`。`permissions.sh` は、このグループが存在しない場合に作成する。ほかに使用する利用者がいる場合は、削除しない。
3. udev の規則を削除する: `permissions.sh` は、`/etc/udev/rules.d/99-input.rules` が存在しない場合に、このファイルを次の 1 行で作成する。内容がこの 1 行だけの場合はファイルを削除し、ほかの行もある場合は、この行だけを削除する。

```text
KERNEL=="uinput", MODE="0660", GROUP="uinput", OPTIONS+="static_node=uinput"
```
