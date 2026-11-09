#!/usr/bin/env bash
# Stops and removes the user services registered by platform/linux/install.sh: the services, their unit files and the
# installation in ~/.local/share/kanata-nicola/. Keeps the settings (~/.config/kanata-nicola/) and the permissions.
#
# Usage:  platform/linux/uninstall.sh      shows what it removes and asks first
#         platform/linux/uninstall.sh -y   removes without asking (for scripts and tests)
set -euo pipefail

yes=0
for arg in "$@"; do
  case "$arg" in
    -y | --yes) yes=1 ;;
    *) echo "unknown argument: $arg (usage: platform/linux/uninstall.sh [-y])" >&2; exit 2 ;;
  esac
done

units="$HOME/.config/systemd/user"
dest="$HOME/.local/share/kanata-nicola"
echo "This removes kanata-nicola's user services:"
echo "  - stops and disables kanata-nicola and kanata-nicola-imesync"
echo "  - $units/kanata-nicola.service and kanata-nicola-imesync.service"
echo "  - $dest/"
echo "It keeps the settings ($HOME/.config/kanata-nicola/) and the permissions (docs/usage.md §5.2)."
if ((!yes)); then
  # No answer (no terminal, end of input) is no.
  read -rp "Remove? [y/N] " answer || answer=
  if [[ "$answer" != [yY] ]]; then
    echo "Nothing was removed. (To remove without asking: platform/linux/uninstall.sh -y)"
    exit 1
  fi
fi

systemctl --user disable --now kanata-nicola-imesync.service kanata-nicola.service 2>/dev/null || true
rm -f "$units/kanata-nicola.service" "$units/kanata-nicola-imesync.service"
systemctl --user daemon-reload
rm -rf "$dest"
echo "Removed."
