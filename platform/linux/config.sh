# shellcheck shell=bash
# Builds the arguments for the generator (nicola-kbdgen) and for kanata from the user's settings (~/.config/kanata-nicola/config.env).
# Shared by platform/linux/run.sh (manual start), platform/linux/install.sh, and service.sh, which the user services
# run at every start (install.sh installs both scripts in ~/.local/share/kanata-nicola/).
# The full list of settings is in docs/settings.md.
#
# Usage:  source platform/linux/config.sh; nicola_config || exit 1   (from the repository root, or the installation)
# Result: gen_args (generator arguments), kanata_args (kanata arguments), ime_sync (0/1), port
#         nicola_config returns 1 (with a message) if a setting has an invalid value.

# Where the user's settings live (outside the repository).
nicola_config_dir="${XDG_CONFIG_HOME:-$HOME/.config}/kanata-nicola"

# The settings. docs/settings.md and config.env.example list the same ones (a test compares them).
nicola_settings=(NICOLA_LAYOUT_OVERLAYS NICOLA_EXTRA_KBD NICOLA_THUMB_KEYS NICOLA_KEYS_AS_CTRL NICOLA_IME_SYNC
                 NICOLA_INCLUDE_DEVICES NICOLA_EXCLUDE_DEVICES NICOLA_KANATA_PORT NICOLA_KANATA_BIN NICOLA_WAIT_DEVICE_MS)

# NICOLA_KANATA_PORT when not set.
nicola_default_port=23800

# Turns a layout overlay file setting into an actual path.
#   Name only (e.g. lag-tolerant)           the bundled layouts/overlays/<name>.toml (of the repository or the installation)
#   Absolute path                           used as is
#   Anything else (e.g. user-layout.toml)  relative to the settings directory
nicola_overlay_path() {
  local x=$1
  if [[ "$x" != */* && "$x" != *.toml ]]; then
    echo "$PWD/layouts/overlays/$x.toml"
  elif [[ "$x" == /* ]]; then
    echo "$x"
  else
    echo "$nicola_config_dir/$x"
  fi
}

# Appends "<option> <item>" to gen_args for each item of a ":"-separated setting. Splits with read,
# so names with *, ? or [ ] are not expanded as file name patterns.
nicola_add_each() {
  local option=$1 item items
  IFS=: read -ra items <<<"$2"
  for item in "${items[@]}"; do
    if [[ -n "$item" ]]; then
      [[ "$option" == --overlay ]] && item=$(nicola_overlay_path "$item")
      gen_args+=("$option" "$item")
    fi
  done
}

# Checks that setting $1 (default $2) is a decimal number from $3 to $4; $5 says what it is.
nicola_check_number() {
  local v=${!1:-$2}
  if ! [[ "$v" =~ ^[0-9]{1,5}$ ]] || ((10#$v < $3 || 10#$v > $4)); then
    echo "$1 must be $5 ($3-$4): $v" >&2
    return 1
  fi
}

# Checks the values that the scripts use directly, so that a mistake names the setting.
nicola_check_values() {
  [[ "${NICOLA_IME_SYNC:-0}" == [01] ]] || { echo "NICOLA_IME_SYNC must be 0 or 1: $NICOLA_IME_SYNC" >&2; return 1; }
  # Two names, left:right (the generator checks the names themselves).
  if [[ -n "${NICOLA_THUMB_KEYS:-}" && ! "$NICOLA_THUMB_KEYS" =~ ^[^:]+:[^:]+$ ]]; then
    echo "NICOLA_THUMB_KEYS must be the left and the right thumb keys, separated by \":\" (e.g. KEY_SPACE:KEY_HENKAN): $NICOLA_THUMB_KEYS" >&2
    return 1
  fi
  nicola_check_number NICOLA_WAIT_DEVICE_MS 0 0 99999 "a number of milliseconds" || return 1
  nicola_check_number NICOLA_KANATA_PORT "$nicola_default_port" 1 65535 "a port number" || return 1
}

# Reads config.env. It is run as a shell script (docs/settings.md); it is first tried in a separate
# shell that stops at the first error, so that a mistake (a syntax error, or "NAME = value", which runs
# NAME as a command) stops here instead of leaving that setting at its default. That shell also stops on
# a variable that is not set (-u), as the scripts that source config.env do: they would end at once there,
# without saying why (install.sh) or with another exit status than 78 (service.sh).
nicola_load_config() {
  local file=$1 err name
  if ! err=$(bash -eu -c 'source "$1"' config.env "$file" 2>&1 >/dev/null); then
    echo "error in $file (each setting is NAME=value, without spaces around =; the variables it uses must be set):" >&2
    echo "$err" >&2
    return 1
  fi
  # shellcheck source=/dev/null  # the user's file
  source "$file"
  # A misspelt setting would be ignored without a word.
  while read -r name; do
    [[ " ${nicola_settings[*]} " == *" $name "* ]] ||
      echo "warning: $file: $name is not a setting, so it is ignored (the settings are in docs/settings.md)" >&2
  done < <(grep -oE '^[[:space:]]*(export[[:space:]]+)?NICOLA_[A-Za-z0-9_]*=' "$file" | grep -oE 'NICOLA_[A-Za-z0-9_]*')
}

nicola_config() {
  if [[ -f "$nicola_config_dir/config.env" ]]; then
    nicola_load_config "$nicola_config_dir/config.env" || return 1
  fi
  nicola_check_values || return 1

  # Not grabbed: the virtual keyboards of the E2E tests (nicola-e2e), its input and the output of its kanata (kanata
  # leaves out only output devices named "kanata"), so that the tests can run while the user service is running.
  gen_args=(--exclude-device nicola-e2e-input --exclude-device nicola-e2e-output)
  nicola_add_each --include-device "${NICOLA_INCLUDE_DEVICES:-}"
  nicola_add_each --exclude-device "${NICOLA_EXCLUDE_DEVICES:-}"
  nicola_add_each --overlay "${NICOLA_LAYOUT_OVERLAYS:-}"
  # The user's own kanata file: an absolute path, or one relative to the settings directory. The generator
  # checks that it exists (kanata would ignore a missing one).
  if [[ -n "${NICOLA_EXTRA_KBD:-}" ]]; then
    local extra=$NICOLA_EXTRA_KBD
    [[ "$extra" == /* ]] || extra="$nicola_config_dir/$extra"
    gen_args+=(--extra-kbd "$extra")
  fi
  nicola_add_each --key-as-ctrl "${NICOLA_KEYS_AS_CTRL:-}"
  if [[ -n "${NICOLA_THUMB_KEYS:-}" ]]; then
    gen_args+=(--thumbs "${NICOLA_THUMB_KEYS%%:*}" "${NICOLA_THUMB_KEYS#*:}") # checked above
  fi

  port=$((10#${NICOLA_KANATA_PORT:-$nicola_default_port})) # checked above; read as decimal (09999 -> 9999)
  ime_sync="${NICOLA_IME_SYNC:-0}"
  kanata_args=()
  # How long kanata sleeps before each rescan of /dev/input, which stops all input meanwhile. Unset: 0 for our patched
  # kanata, which also rescans when udev sets a device's permissions; kanata's default 200 ms for another one
  # (NICOLA_KANATA_BIN), which without that fix needs the wait to open a keyboard that was just connected.
  if [[ -n "${NICOLA_WAIT_DEVICE_MS:-}" ]]; then
    kanata_args+=(--wait-device-ms "$((10#$NICOLA_WAIT_DEVICE_MS))")
  elif [[ -z "${NICOLA_KANATA_BIN:-}" ]]; then
    kanata_args+=(--wait-device-ms 0)
  fi
  if [[ "$ime_sync" == 1 ]]; then
    # Keys go to the IME unchanged; nicola-imesync switches the layer.
    gen_args+=(--toggle none)
    kanata_args+=(-p "127.0.0.1:$port")
  fi
  nicola_check_caps_ctrl
}

# Notes the way of switching NICOLA that suits the input method running in this session (install.sh): IME sync with
# fcitx5, the key toggle with ibus (IME sync follows fcitx5 only, so NICOLA would stay off). Silent when neither runs
# (e.g. over ssh). Uses ime_sync from nicola_config.
nicola_note_ime() {
  local me
  me=$(id -u)
  if pgrep -u "$me" -x fcitx5 >/dev/null; then
    [[ "$ime_sync" == 1 ]] && return 0
    echo "note: fcitx5 is running: NICOLA_IME_SYNC=1 is recommended, so that NICOLA follows the IME (docs/usage.md §3.1)" >&2
  elif [[ "$ime_sync" == 1 ]] && pgrep -u "$me" -x ibus-daemon >/dev/null; then
    echo "note: the input method running is ibus, but NICOLA_IME_SYNC=1 follows fcitx5 only, so NICOLA would stay off." >&2
    echo "  With ibus, use the key toggle: NICOLA_IME_SYNC=0 (docs/usage.md §2.2)" >&2
  fi
  return 0
}

# Warns if the desktop's keyboard settings make CapsLock a Ctrl key but NICOLA_KEYS_AS_CTRL does not: kanata reads the
# keys before those settings, so it sees CapsLock, and with NICOLA on CapsLock+C would give Ctrl with the keys of a
# character. Only GNOME's settings are read; nothing is said where they cannot be read.
nicola_check_caps_ctrl() {
  local options
  [[ ":${NICOLA_KEYS_AS_CTRL:-}:" == *:KEY_CAPSLOCK:* ]] && return 0
  options=$(gsettings get org.gnome.desktop.input-sources xkb-options 2>/dev/null) || return 0
  # ctrl:nocaps, ctrl:swapcaps and its variants (_hyper, _and_switch_layout), caps:ctrl_modifier.
  if [[ "$options" =~ ctrl:nocaps|ctrl:swapcaps|caps:ctrl_modifier ]]; then
    echo "warning: CapsLock is Ctrl in the desktop's keyboard settings (xkb-options $options), but not in NICOLA_KEYS_AS_CTRL:" \
      "with NICOLA on, CapsLock+C gives Ctrl with the keys of a character instead of Ctrl+C." \
      "Set NICOLA_KEYS_AS_CTRL=KEY_CAPSLOCK in config.env (docs/settings.md)" >&2
  fi
}

# Our patched build carries three fixes for keyboard reconnects (platform/linux/kanata/README.md):
# 0001 = upstream #2129, in releases from this version on; 0002 = rescan when udev sets a device
# node's permissions, and 0003 = do not drop queued key events at a rescan (a bug of 0001), in no release yet.
# And 0004: its TCP port (IME sync) takes ChangeLayer and requests only, not the commands that load another
# configuration or act on virtual keys, which any local process could send.
NICOLA_KANATA_FIXED_VERSION=1.13.0
NICOLA_KANATA_PATCHED=target/kanata-patched/kanata

# The kanata to use: NICOLA_KANATA_BIN if set, otherwise our build of kanata with the upstream fix
# (platform/linux/kanata/README.md) if present, otherwise kanata on PATH. Returns 1 (with a message) if none.
nicola_kanata_bin() {
  if [[ -n "${NICOLA_KANATA_BIN:-}" ]]; then
    if [[ ! -f "$NICOLA_KANATA_BIN" || ! -x "$NICOLA_KANATA_BIN" ]]; then
      echo "NICOLA_KANATA_BIN is not an executable: $NICOLA_KANATA_BIN" >&2
      return 1
    fi
    echo "$NICOLA_KANATA_BIN"
  elif [[ -x "$NICOLA_KANATA_PATCHED" ]]; then
    echo "$PWD/$NICOLA_KANATA_PATCHED"
  elif ! command -v kanata; then
    echo "kanata not found: build it with platform/linux/kanata/build.sh (install.sh does), or set NICOLA_KANATA_BIN" >&2
    return 1
  fi
}

# Whether our patched build exists and was built from the current base version and patches.
nicola_kanata_patched_current() {
  [[ -x "$NICOLA_KANATA_PATCHED" ]] &&
    [[ "$(cat "$NICOLA_KANATA_PATCHED.stamp" 2>/dev/null)" == "$(platform/linux/kanata/build.sh --stamp)" ]]
}

# Warns (and returns 1) if the given kanata lacks the reconnect fixes. Our patched build has them.
nicola_check_kanata_version() {
  local bin=$1 version
  if [[ "$bin" == "$PWD/$NICOLA_KANATA_PATCHED" ]]; then
    nicola_kanata_patched_current && return 0
    echo "warning: $bin was built from other patches than the current ones (platform/linux/kanata/patches/);" >&2
    echo "  rebuild it with platform/linux/kanata/build.sh (install.sh does)." >&2
    return 1
  fi
  version=$("$bin" --version 2>/dev/null | awk '{print $2}')
  if [[ -z "$version" ]]; then
    echo "warning: cannot read the version of $bin" >&2
    return 1
  fi
  if [[ "$(printf '%s\n%s\n' "$NICOLA_KANATA_FIXED_VERSION" "$version" | sort -V | head -1)" != "$NICOLA_KANATA_FIXED_VERSION" ]]; then
    echo "warning: kanata $version ($bin) lacks the upstream fix for repeated reconnects (#2129, in $NICOLA_KANATA_FIXED_VERSION and later):" >&2
    echo "  after a keyboard is reconnected a second time (unplug, resume from sleep), NICOLA stops working on it." >&2
  fi
  echo "warning: kanata $version ($bin) is not our patched build, which also rescans when udev sets a device's" >&2
  echo "  permissions (without it, a keyboard connected right after resume can stay without NICOLA), does not" >&2
  echo "  drop key events when it rescans (with #2129 alone, a key can stay held when another device connects)," >&2
  echo "  and with IME sync takes only layer changes and queries on its TCP port (any local process can connect;" >&2
  echo "  through another kanata's port, it can load a configuration of its own, which can also read the keys)." >&2
  echo "  Unset NICOLA_KANATA_BIN to use our patched build (platform/linux/kanata/README.md)." >&2
  return 1
}

# Checks that the tools for the build are there (docs/usage.md), so that install.sh says what is missing before a
# build fails halfway. Returns 1 after listing what is missing and the packages to install on Ubuntu.
nicola_check_build_tools() {
  local missing=() c
  command -v cargo >/dev/null ||
    missing+=("cargo (Rust: https://rustup.rs; in a terminal opened before installing it, first: source ~/.cargo/env)")
  command -v cc >/dev/null || missing+=("cc (a C compiler and linker)")
  if ! command -v pkg-config >/dev/null; then
    missing+=("pkg-config")
  elif ! pkg-config --exists dbus-1; then
    missing+=("the D-Bus development files (dbus-1)")
  fi
  # For our build of kanata: its source is downloaded and patched.
  for c in curl patch; do
    command -v "$c" >/dev/null || missing+=("$c")
  done
  ((${#missing[@]} == 0)) && return 0
  echo "Missing for the build (docs/usage.md §2):" >&2
  printf '  - %s\n' "${missing[@]}" >&2
  echo "On Ubuntu: sudo apt install build-essential pkg-config libdbus-1-dev curl patch git" >&2
  return 1
}

# Checks that no other kanata is running (it would hold the keyboards). Returns 1 if one is.
nicola_check_conflicts() {
  local pids others
  # Also builds named otherwise (kanata_cmd_allowed etc.).
  pids=$(pgrep '^kanata') || return 0
  # Another user's: their kanata-nicola keeps the keyboards when they switch users without logging out.
  others=$(ps -o uid= -p "${pids//$'\n'/,}" | awk -v me="$(id -u)" '$1 != me { print $1 }' | sort -u |
    while read -r uid; do { getent passwd "$uid" || echo "uid $uid"; } | cut -d: -f1; done | paste -sd ' ') || true
  if [[ -n "$others" ]]; then
    echo "kanata of another user ($others) is running and holds the keyboards: their kanata-nicola, if they switched" >&2
    echo "  users without logging out (not supported: docs/usage.md §3.2). Have them log out, then start again" >&2
    echo "  (the user service: systemctl --user restart kanata-nicola)." >&2
    return 1
  fi
  echo "kanata is already running (platform/linux/run.sh or the user service). Stop it." >&2
  echo "  To stop the user service: systemctl --user stop kanata-nicola.service" >&2
  return 1
}
