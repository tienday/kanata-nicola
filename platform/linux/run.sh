#!/usr/bin/env bash
# Starts kanata-nicola manually, to try it in a terminal (docs/implementation.md).
#
# Settings: the NICOLA_ environment variables, or the same lines in ~/.config/kanata-nicola/config.env to load them
# every time. The list is in docs/settings.md; example: platform/linux/config.env.example.
#
# Stop: Ctrl+C in this terminal. If that does not work, Left Ctrl + Space + Esc (kanata's emergency stop).
set -euo pipefail
cd "$(dirname "$0")/../.."

source platform/linux/config.sh
nicola_config || exit 1
nicola_check_conflicts || exit 1

out=target/run
mkdir -p "$out"

cargo run -q --release -p nicola-kbdgen -- "${gen_args[@]}" -o "$out/nicola.kbd"
kanata_bin=$(nicola_kanata_bin) || exit 1
nicola_check_kanata_version "$kanata_bin" || true
"$kanata_bin" --check -c "$out/nicola.kbd" >/dev/null 2>&1 || { "$kanata_bin" --check -c "$out/nicola.kbd"; exit 1; }

log="$out/kanata-$(date +%Y%m%d-%H%M%S).log"
if [[ "$ime_sync" == 1 ]]; then
  cargo build -q --release -p nicola-imesync
  ./target/release/nicola-imesync --port "$port" >"$out/imesync-$(date +%Y%m%d-%H%M%S).log" 2>&1 &
  sync_pid=$!
  trap 'kill $sync_pid 2>/dev/null' EXIT
fi

cat <<MSG
Config: $out/nicola.kbd
Log:    $log
- Do not touch any keys for the first few seconds (kanata waits before it grabs the keyboard).
$(if [[ "$ime_sync" == 1 ]]; then echo "- IME sync: NICOLA follows the IME state automatically (hiragana/katakana in fcitx5 + Mozc)."; else echo "- NICOLA starts OFF. Hankaku/Zenkaku turns the IME and NICOLA ON together."; fi)
- Stop: Ctrl+C in this terminal / Emergency stop: Left Ctrl + Space + Esc
MSG
"$kanata_bin" -c "$out/nicola.kbd" "${kanata_args[@]}" --log-layer-changes 2>&1 | tee "$log"
