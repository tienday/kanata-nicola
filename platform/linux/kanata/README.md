# 修正を適用した kanata

この文書は、kanata-nicola が Linux で、キー割り当てツール [kanata](https://github.com/jtroo/kanata) に適用する修正と、修正を適用した kanata のビルドの方法を説明する。kanata-nicola が修正を適用した kanata を使用する理由と、kanata の仕様・制約は、[設計書 §10](../../../docs/design.md#10-kanata-の仕様と修正) にある。

| 項目 | 内容 |
|---|---|
| 基にするバージョン | kanata 1.11.0（crates.io のソース） |
| 修正 | キーボードの再接続に関わる 3 つ（0001〜0003）と、TCP ポートで受け付ける命令を制限する 1 つ（0004） |
| ライセンス | kanata は LGPL-3.0-only である。`patches/` のパッチは kanata の改変なので、リポジトリのほかの部分と異なり、kanata と同じ LGPL-3.0-only とする（本文は `patches/COPYING.LESSER`、LGPL-3.0 が引用する GPL-3.0 の本文は `patches/COPYING`）。0001 は上流（jtroo/kanata）のコミットなので、その作者の表示を残している |

## 適用する修正

### 0001: 繰り返しの再接続への追従（上流の修正）

| 項目 | 内容 |
|---|---|
| パッチ | `patches/0001-fix-linux-detect-repeated-device-hotplug-2129.patch` |
| 不具合 | Linux で、キーボードの再接続（抜き差し、スリープ復帰での USB・Bluetooth キーボードの再接続）に、起動後の最初の 1 回しか追従しない |
| 原因 | `/dev/input` の inotify の通知を読み出さないため、エッジトリガの epoll に、2 回目以降の通知が届かない |
| 修正 | 上流の [#2129](https://github.com/jtroo/kanata/pull/2129) のコミット d627fe68d5「fix(linux): detect repeated device hotplug (#2129)」（作者 Sergey Lukashevich）を、変更せずに適用する。inotify の通知を毎回読み出し、再探索の前に、取り外されたデバイスを登録から削除する |
| 上流の状況 | v1.13.0-prerelease-1 に含まれている。正式版（v1.12.0 まで）には含まれていない |

### 0002: 権限の変更時の再探索（独自の修正）

| 項目 | 内容 |
|---|---|
| パッチ | `patches/0002-fix-linux-rescan-on-device-permission-change.patch` |
| 不具合 | 接続した直後のキーボードを、別のデバイスが接続されるまで取得しない。その間、そのキーボードの入力は kanata を通らない。スリープ復帰の直後や、USB ハブで複数のデバイスが同時に接続されるときに起きやすい |
| 原因 | キーボードのデバイスファイルは、カーネルが root だけが読める形で作成し、少し後に udev が `input` グループの権限を付ける。kanata は、作成の通知から `--wait-device-ms`（既定 200ms）後に 1 回だけ探し、udev がそれより遅いと、開けないキーボードを、ログに記録せずに無視する |
| 修正 | `/dev/input` の inotify で、作成（CREATE）に加えて属性の変更（ATTRIB）も監視し、udev が権限を付けたときにも探し直す |
| 上流の状況 | 上流（v1.13.0-prerelease-1 まで）も作成（CREATE）だけを監視しており、未修正 |
| 確認の方法 | E2E（実際の kanata のプロセスを起動して行う試験。[実装仕様書 §6.3](../../../docs/implementation.md#63-e2e)）の抜き差しの確認を、kanata の待ち時間 0ms（E2E の既定）で実行する。修正の無い kanata はキーボードを取得し直さず、修正を適用した kanata は毎回 0.5 秒程度で取得し直す |

### 0003: キーのイベントを失わない取り外しの判定（独自の修正）

| 項目 | 内容 |
|---|---|
| パッチ | `patches/0003-fix-linux-detect-removed-devices-without-dropping-key-events.patch` |
| 不具合 | 再探索の直前に届いたキーの押下・離しが失われる。離しが失われると、kanata の中でそのキーが押されたままになる。再探索は `/dev/input` の inotify の通知のたびに行われるので、いずれかの入力デバイスを接続するたびに、すべてのキーボードで起きうる |
| 原因 | 0001 が加えた取り外しの判定（`remove_stale_devices()`）は、再探索のたびに、取得中のすべてのキーボードに EVIOCGKEY（`get_key_state()`）を呼び出し、ENODEV（デバイスが取り外された）かを調べる。Linux の evdev は、EVIOCGKEY を受けると、呼び出し元の fd の、まだ読み取られていないキーのイベントを、通知せずに破棄する。kanata は、同じ fd でキーボードを読み取っている |
| 修正 | 取り外しの判定を、デバイスの ID を読み取るだけの EVIOCGID に変更する。evdev の ioctl は、どれもデバイスが取り外されている場合は ENODEV で失敗するので、判定の結果は変わらない |
| 上流の状況 | 上流（v1.13.0-prerelease-1 まで）も EVIOCGKEY を使用しており、未修正 |
| 確認の方法 | キーボードを取得し、キーの押下と離しのイベントを読み取らずに残した状態で EVIOCGKEY を呼び出すと 2 件とも破棄され、EVIOCGID では残ることを、小さなプログラムで確認した。E2E では、再探索とキーの到着が重なる状況を作れないため、確認していない |

### 0004: TCP ポートで受け付ける命令の制限（独自の修正）

| 項目 | 内容 |
|---|---|
| パッチ | `patches/0004-tcp-server-take-changelayer-and-requests-only.patch` |
| 理由 | kanata の TCP サーバには認証が無く、同じ PC のプロセスは、利用者を問わず接続できる。修正の無い kanata は、設定ファイルを読み込ませる命令（`ReloadFile`・`Reload…`）、仮想キーを操作する命令（`ActOnFakeKey`）、マウスを操作する命令（`SetMouse`）も受け付け、読み込ませた設定で、キー入力を差し替えることも読み取ることもできる（[設計書 §10.5](../../../docs/design.md#105-tcp-ポート)）。IME 同期が送信する命令は `ChangeLayer` だけである |
| 修正 | TCP サーバが受け付ける命令を、レイヤの切り替え（`ChangeLayer`）と、状態を変えない問い合わせ（`Hello`・`Request…`）に限る。ほかの命令にはエラーを返し、接続を切断する（不正な形の命令と同じ扱い） |
| 上流の状況 | 上流（v1.13.0-prerelease-1 まで）には、受け付ける命令を制限する設定が無い |
| 確認の方法 | E2E の「tcp port」の項目で、`ChangeLayer` でレイヤが切り替わって通知を受け取ること、`ReloadFile`・`ActOnFakeKey` にエラーが返って接続が切断されることを確認する。修正の無い kanata では、後の 2 つが失敗する |

## ビルド

`build.sh` は、crates.io から基にするバージョンのソースを取得して SHA-256 を照合し、`patches/` のパッチを番号順に適用してビルドする。次のコマンドは、リポジトリの直下で実行する。

```sh
platform/linux/kanata/build.sh           # ビルドする（出力は target/kanata-patched/kanata）
platform/linux/kanata/build.sh --stamp   # 基にするバージョンとパッチの SHA-256 を表示する
```

**再ビルド:** `build.sh` は、ビルドした kanata の横に、`--stamp` の出力を `kanata.stamp` として記録する。`install.sh` は、ビルドした kanata が無い場合と、`kanata.stamp` が現在の `--stamp` の出力と一致しない場合（基にするバージョンやパッチを変更した場合）に、`build.sh` を実行する。`NICOLA_KANATA_BIN` で別の kanata を指定した場合は、実行しない。
