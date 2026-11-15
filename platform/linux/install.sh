#!/usr/bin/env bash
# Installs or updates kanata-nicola as systemd user services (start at login, restart after an abnormal exit).
#
# Usage (from anywhere in the repository):
#   platform/linux/install.sh          Build, install, register the services. If they are running, restart them
#   platform/linux/install.sh --start  The above, plus start them if stopped (run.sh must not be running)
#
# Run it to install, and again after updating the repository. Not for a change of settings: the services read
# ~/.config/kanata-nicola/config.env (docs/settings.md) at every start, so  systemctl --user restart kanata-nicola
# applies it.
# Installed to: ~/.local/share/kanata-nicola/ (programs; replaced as a whole, not affected by development builds)
# Logs:         journalctl --user -u kanata-nicola -u kanata-nicola-imesync
# Stop:         systemctl --user stop kanata-nicola   (stops kanata-nicola-imesync too)
# kanata stopped by the emergency stop (Left Ctrl + Space + Esc) is not restarted. To resume: systemctl --user restart kanata-nicola
# The last line says how it ended: installed and running, installed and not running yet, or installed but not running
# (exit status 1).
set -euo pipefail

start=0
for arg in "$@"; do
  case "$arg" in
    --start) start=1 ;;
    *) echo "unknown argument: $arg (usage: platform/linux/install.sh [--start])" >&2; exit 1 ;;
  esac
done

cd "$(dirname "$0")/../.."
repo=$(pwd)

# Like the services, use the settings of config.env only.
if [[ -n "${!NICOLA_*}" ]]; then
  echo "note: not using ${!NICOLA_*} of the environment: the services read config.env only" >&2
  unset "${!NICOLA_@}"
fi
source platform/linux/config.sh
source platform/linux/install-files.sh

echo "== permissions"
platform/linux/permissions.sh --check || exit 1
# Stop on a mistake in the settings before building. Notes about them are shown by the check after the build.
if ! nicola_config 2>/dev/null; then
  nicola_config
  exit 1
fi

dest="$HOME/.local/share/kanata-nicola"
units="$HOME/.config/systemd/user"

echo "== build"
nicola_check_build_tools || exit 1
cargo build -q --release -p nicola-kbdgen -p nicola-imesync
# kanata: our build of kanata with the fixes for keyboard reconnects (platform/linux/kanata/README.md), built the
# first time and when the base version or the patches changed. None when NICOLA_KANATA_BIN names another kanata.
kanata=
if [[ -z "${NICOLA_KANATA_BIN:-}" ]]; then
  if ! nicola_kanata_patched_current; then
    echo "== build kanata (patched; takes a few minutes)"
    platform/linux/kanata/build.sh
  fi
  kanata=$repo/$NICOLA_KANATA_PATCHED
fi

echo "== install"
# Into a new directory first, so that a failure leaves the installed one as it was.
trap 'rm -rf "$dest.new"' EXIT
rm -rf "$dest.new"
nicola_install_files "$repo" target/release "$dest.new" "$kanata"
"$dest.new/service.sh" check "$nicola_config_dir"
# Both are made before anything is replaced, so that a refused path leaves the old installation in place.
unit=$(nicola_unit_kanata "$repo" "$dest" "$nicola_config_dir") || exit 1
imesync_unit=$(nicola_unit_imesync "$repo" "$dest" "$nicola_config_dir") || exit 1
rm -rf "$dest.old"
if [[ -e "$dest" ]]; then
  mv "$dest" "$dest.old"
fi
mv "$dest.new" "$dest"
rm -rf "$dest.old"

echo "== units"
mkdir -p "$units"
printf '%s\n' "$unit" >"$units/kanata-nicola.service"
printf '%s\n' "$imesync_unit" >"$units/kanata-nicola-imesync.service"
systemctl --user daemon-reload
# reenable: enable alone would keep links that the units no longer ask for (default.target.wants/ of older versions).
systemctl --user -q reenable kanata-nicola.service kanata-nicola-imesync.service

# Restarting or starting kanata-nicola does the same to nicola-imesync (which runs only with IME sync on).
started=0
if systemctl --user is-active --quiet kanata-nicola.service; then
  echo "== restart"
  systemctl --user restart kanata-nicola.service
  started=1
elif [[ "$start" == 1 ]]; then
  nicola_check_conflicts || exit 1
  echo "== start"
  systemctl --user start kanata-nicola.service
  started=1
fi
if ((started)); then
  # kanata takes the keyboards (and opens the TCP port) about 2 seconds after it starts. Wait until it says it runs, or
  # the service stops (a crash can take another second or more, while the core dump is written); 15 seconds at most.
  id=$(systemctl --user show -p InvocationID --value kanata-nicola.service)
  for _ in $(seq 30); do
    systemctl --user is-active --quiet kanata-nicola.service || break
    # Not "| grep -q": with pipefail, journalctl ending on SIGPIPE after grep found it would count as not found.
    [[ "$(journalctl --user -q -o cat _SYSTEMD_INVOCATION_ID="$id")" == *'Starting kanata proper'* ]] && break
    sleep 0.5
  done
fi
# The first line of each (after the state mark) and its state.
systemctl --user --no-pager --lines=0 status kanata-nicola.service kanata-nicola-imesync.service 2>/dev/null |
  grep -E '^[^ ]+ kanata-nicola|Active:' || true
nicola_note_ime

echo "== done"
# The version installed, and the commit it came from (git describe; none outside a git working tree).
commit=$(git -C "$repo" describe --tags --always --dirty 2>/dev/null || true)
echo "$(nicola_version "$dest")${commit:+ (git $commit)}"
if ((!started)); then
  echo "Installed. Not running yet: it starts at the next login. To start it now: systemctl --user restart kanata-nicola"
elif systemctl --user is-active --quiet kanata-nicola.service; then
  echo "Installed and running."
else
  echo "Installed, but kanata-nicola is not running. The reason: systemctl --user status kanata-nicola" >&2
  echo "  (docs/usage.md §4)" >&2
  exit 1
fi
