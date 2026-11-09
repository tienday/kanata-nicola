#!/usr/bin/env bash
# Builds kanata (the version below) with the fixes in patches/ applied (see README.md for why).
#
# Usage:  platform/linux/kanata/build.sh           build
#         platform/linux/kanata/build.sh --stamp   print what a build is made from (base version, patch checksums)
# Output: target/kanata-patched/kanata (found and used by platform/linux/install.sh), and kanata.stamp next to
#         it (the --stamp output at build time; install.sh rebuilds when it no longer matches)
set -euo pipefail
cd "$(dirname "$0")/../../.."

# The same version as the kanata dev-dependency of crates/nicola-kbdgen, which the simulation tests run (a test
# compares them).
version=1.11.0
# SHA-256 of that version's .crate on crates.io (same as the value in cargo's registry)
sha256=256eb31e07bc8768982e6206344fdd70cdb4a7f5b255930ab66bf2bc5741bb8f
work=target/kanata-patched
crate="$work/kanata-$version.crate"
src="$work/kanata-$version"
patches=(platform/linux/kanata/patches/*.patch)

stamp() {
  echo "kanata $version $sha256"
  sha256sum "${patches[@]}"
}
if [[ "${1:-}" == "--stamp" ]]; then
  stamp
  exit 0
fi

mkdir -p "$work"
rm -f "$work/kanata.stamp"
# Fetch into a temporary name and keep it only if the checksum matches, so that a broken download is
# fetched again next time instead of failing every time. (The sha256sum of uutils, Ubuntu 26.04's coreutils,
# reports a missing file on standard output even with --status.)
if ! echo "$sha256  $crate" | sha256sum -c --status >/dev/null 2>&1; then
  rm -f "$crate"
  cached=("$HOME"/.cargo/registry/cache/*/"kanata-$version.crate")
  if [[ -f "${cached[0]}" ]]; then
    cp "${cached[0]}" "$crate.part"
  else
    curl -sSfL "https://static.crates.io/crates/kanata/kanata-$version.crate" -o "$crate.part"
  fi
  if ! echo "$sha256  $crate.part" | sha256sum -c --status >/dev/null; then
    rm -f "$crate.part"
    echo "kanata-$version.crate does not match the expected SHA-256 ($sha256); not building" >&2
    exit 1
  fi
  mv "$crate.part" "$crate"
fi

rm -rf "$src"
tar xzf "$crate" -C "$work"
for p in "${patches[@]}"; do
  echo "apply $(basename "$p")"
  patch -s -p1 -d "$src" <"$p"
done

# Build with the dependency versions in kanata's bundled Cargo.lock and the same default features as cargo install kanata.
CARGO_TARGET_DIR="$work/build" cargo build -q --release --locked --manifest-path "$src/Cargo.toml"
install -m 755 "$work/build/release/kanata" "$work/kanata"
stamp >"$work/kanata.stamp"
echo "built $work/kanata ($("$work/kanata" --version)) + ${#patches[@]} patch(es)"
