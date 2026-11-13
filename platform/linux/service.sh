#!/usr/bin/env bash
# What the systemd user units of kanata-nicola run (platform/linux/install-files.sh). install.sh installs it with
# the programs in ~/.local/share/kanata-nicola/. It reads the settings at every start, so that
#   systemctl --user restart kanata-nicola
# applies a change of config.env (to nicola-imesync too, which the unit restarts with it).
#
# Usage:  service.sh kanata <settings dir>       make the kanata configuration from the settings, check it, run kanata
#         service.sh check <settings dir>        the same without running kanata (install.sh checks the settings with it)
#         service.sh ime-sync-on <settings dir>  exit 0 if the settings turn IME sync on, 1 if not (ExecCondition=)
#         service.sh imesync <settings dir>      run nicola-imesync
# Exits with 78 (EX_CONFIG) on a mistake in the settings, or when another kanata is running, after which
# the unit is not restarted.
set -euo pipefail

if [[ $# != 2 ]]; then
  echo "usage: $0 kanata|check|ime-sync-on|imesync <settings dir>" >&2
  exit 2
fi
mode=$1
# The installation directory: the bundled overlays are in layouts/overlays/ here, as in the repository.
cd "$(dirname "$0")"
# The settings come from config.env only, not from the environment of the systemd user manager.
unset "${!NICOLA_@}"
# shellcheck source-path=SCRIPTDIR
source ./config.sh
nicola_config_dir=$2

case "$mode" in
  ime-sync-on)
    # A mistake in the settings is reported by kanata-nicola, which reads the same ones.
    if nicola_config 2>/dev/null && [[ "$ime_sync" == 1 ]]; then
      exit 0
    fi
    exit 1
    ;;
  imesync)
    nicola_config 2>/dev/null || exit 78
    exec ./nicola-imesync --port "$port"
    ;;
  kanata | check) ;;
  *)
    echo "unknown command: $mode (usage: $0 kanata|check|ime-sync-on|imesync <settings dir>)" >&2
    exit 2
    ;;
esac

if [[ -f "$nicola_config_dir/config.env" ]]; then
  echo "settings: $nicola_config_dir/config.env"
else
  echo "settings: none ($nicola_config_dir/config.env does not exist), so the defaults"
fi
# Which version runs, for the log of the unit (journalctl, systemctl status). Not in check: install.sh shows it.
if [[ "$mode" == kanata ]]; then
  nicola_version .
fi
nicola_config || exit 78
# The unit's own kanata has exited by now, so a kanata running is another one: most likely that of another user who
# switched users without logging out. This one could not take the keyboards (kanata keeps running without them, or
# exits when the TCP port is taken), so say why and do not restart (78).
if [[ "$mode" == kanata ]]; then
  nicola_check_conflicts || exit 78
fi

# The kanata to run: NICOLA_KANATA_BIN if set, otherwise the one install.sh installed here.
if [[ -n "${NICOLA_KANATA_BIN:-}" ]]; then
  kanata=$(nicola_kanata_bin) || exit 78
  nicola_check_kanata_version "$kanata" || true
elif [[ -x kanata ]]; then
  kanata=$PWD/kanata
else
  echo "no kanata in $PWD: install.sh installs it when NICOLA_KANATA_BIN is not set; run platform/linux/install.sh" >&2
  exit 78
fi

if [[ "$mode" == check ]]; then
  out=$(mktemp -d)
  trap 'rm -rf "$out"' EXIT
else
  # The runtime directory of the unit (RuntimeDirectory=), removed when the service stops.
  out=${RUNTIME_DIRECTORY:?is not set (RuntimeDirectory= of the unit)}
fi
./nicola-kbdgen "${gen_args[@]}" -o "$out/nicola.kbd" || exit 78
if ! err=$("$kanata" --check -c "$out/nicola.kbd" 2>&1); then
  echo "$err" >&2
  exit 78
fi
if [[ "$mode" == check ]]; then
  exit 0
fi
exec "$kanata" -c "$out/nicola.kbd" "${kanata_args[@]}" --no-wait
