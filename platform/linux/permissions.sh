#!/usr/bin/env bash
# Checks that this user can run kanata: read the keyboards (/dev/input/event*) and make the virtual keyboard that
# sends the keys (/dev/uinput). Sets up what is missing with sudo, as kanata's guide for Linux does (docs/usage.md).
#
# Usage:  platform/linux/permissions.sh          check, and set up what is missing (shows the commands and asks first)
#         platform/linux/permissions.sh --check  check only; exit 1 if something is missing (install.sh runs this)
set -euo pipefail

rule_file=/etc/udev/rules.d/99-input.rules
rule='KERNEL=="uinput", MODE="0660", GROUP="uinput", OPTIONS+="static_node=uinput"'

# What the system has. Functions, so that the tests can stand in for them.
# A keyboard: an input device with the kbd handler (/proc/bus/input/devices). Not just any of /dev/input/event*: the
# user at the seat can read some of them without the input group, such as a game controller (systemd's uaccess).
can_read_keyboards() {
  local line handlers h
  while read -r line; do
    [[ "$line" == "H: Handlers="* ]] || continue
    read -ra handlers <<<"${line#H: Handlers=}"
    [[ " ${handlers[*]} " == *" kbd "* ]] || continue
    for h in "${handlers[@]}"; do
      [[ "$h" == event* && -r "/dev/input/$h" ]] && return 0
    done
  done </proc/bus/input/devices
  return 1
}
can_use_uinput() { [[ -r /dev/uinput && -w /dev/uinput ]]; }
# In the group database: takes effect at the next login.
in_group() { [[ " $(id -nG "$(id -un)") " == *" $1 "* ]]; }
# In the groups of this login (of this process).
has_group() { [[ " $(id -nG) " == *" $1 "* ]]; }
group_exists() { getent group "$1" >/dev/null; }
uinput_exists() { [[ -e /dev/uinput ]]; }
# Readable and writable by the uinput group.
uinput_set_up() {
  uinput_exists && [[ "$(stat -c %G /dev/uinput)" == uinput ]] && (((8#$(stat -c %a /dev/uinput) & 8#060) == 8#060))
}
rule_file_exists() { [[ -e "$rule_file" ]]; }

# Sets missing (what this user cannot do), steps (the commands that set it up, in order) and relogin (1 if a
# group is in the database but not yet in this login).
permissions_plan() {
  local user
  user=$(id -un)
  missing=() steps=() relogin=0
  if ! can_read_keyboards; then
    missing+=("cannot read the keyboards (/dev/input/event*): needs the input group")
    in_group input || steps+=("sudo usermod -aG input $user")
    has_group input || relogin=1
  fi
  if ! can_use_uinput; then
    missing+=("cannot use /dev/uinput (to send the keys): needs the uinput group")
    group_exists uinput || steps+=("sudo groupadd --system uinput")
    uinput_exists || steps+=("sudo modprobe uinput")
    if ! uinput_set_up; then
      rule_file_exists || steps+=("echo '$rule' | sudo tee $rule_file >/dev/null")
      steps+=("sudo udevadm control --reload-rules")
      steps+=("sudo udevadm trigger --subsystem-match=misc --sysname-match=uinput")
    fi
    in_group uinput || steps+=("sudo usermod -aG uinput $user")
    has_group uinput || relogin=1
  fi
}

permissions_main() {
  local check=0 answer s
  case "${1:-}" in
    "") ;;
    --check) check=1 ;;
    *) echo "unknown argument: $1 (usage: platform/linux/permissions.sh [--check])" >&2; return 1 ;;
  esac
  permissions_plan
  if ((${#missing[@]} == 0)); then
    echo "ok: this user can read the keyboards and use /dev/uinput"
    return 0
  fi
  echo "kanata needs access that this user does not have (docs/usage.md §2.1):" >&2
  printf '  - %s\n' "${missing[@]}" >&2
  if ((${#steps[@]} == 0)); then
    if ((relogin)); then
      echo "This user is in the groups, but this login started before that: log out and log in again (or restart the PC)." >&2
    else
      echo "This user is in the groups and has them in this login, yet lacks the access: see ls -l /dev/input/event* /dev/uinput" >&2
    fi
    return 1
  fi
  if ((check)); then
    echo "To set it up: platform/linux/permissions.sh (shows the commands and asks first)" >&2
    return 1
  fi
  echo "These commands set it up:"
  printf '  %s\n' "${steps[@]}"
  echo "Note: in the input and uinput groups, every program this user runs can read all key input (passwords too)"
  echo "  and make key input. kanata needs that; do not run programs that you do not trust."
  # No answer (no terminal, end of input) is no.
  read -rp "Run them? [y/N] " answer || answer=
  if [[ "$answer" != [yY] ]]; then
    echo "Nothing was changed."
    return 1
  fi
  for s in "${steps[@]}"; do
    echo "+ $s"
    bash -c "$s"
  done
  # This login is not in the uinput group yet, so look at the device itself (if it was what was missing).
  if ! can_use_uinput && ! uinput_set_up; then
    echo "/dev/uinput is still not readable and writable by the uinput group:" >&2
    ls -l /dev/uinput >&2 || true
    echo "  $rule_file should have the line: $rule" >&2
    return 1
  fi
  if ((relogin)); then
    echo "Done. Log out and log in again (or restart the PC) so that the groups take effect."
  else
    echo "Done."
  fi
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  permissions_main "$@"
fi
