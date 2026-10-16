#!/bin/bash
# Build from source and install for the current user: the binary goes to ~/.local/bin,
# the menu entry and icons to ~/.local/share, the udev rule to /etc/udev/rules.d
# (asks for your password once). `./install.sh --uninstall` removes all of it.
set -euo pipefail
cd "$(dirname "$0")"
here=$(pwd)

id=io.github.f0rd2017.AjazzMouse
bin=$HOME/.local/bin/ajazz-mouse
data=${XDG_DATA_HOME:-$HOME/.local/share}
rule=/etc/udev/rules.d/70-ajazz-mouse.rules

as_root() {
  if [ -t 0 ] && command -v sudo >/dev/null; then sudo "$@"; else pkexec "$@"; fi
}

# A stale icon cache hides new icons from GTK, so refresh it if there is one.
refresh_icons() {
  local cache=$data/icons/hicolor/icon-theme.cache tool
  tool=$(command -v gtk4-update-icon-cache || command -v gtk-update-icon-cache || true)
  if [ -e "$cache" ] && [ -n "$tool" ]; then "$tool" -qtf "$data/icons/hicolor"; fi
}

if [ "${1:-}" = "--uninstall" ]; then
  rm -f "$bin" "$data/applications/$id.desktop" "$data/metainfo/$id.metainfo.xml" "$data"/icons/hicolor/*/apps/$id.png
  refresh_icons
  if [ -e "$rule" ]; then
    as_root sh -c "rm -f '$rule' && udevadm control --reload-rules"
  fi
  echo "Ajazz Mouse is uninstalled."
  exit 0
fi

if ! command -v cargo >/dev/null; then
  echo "Rust is needed to build Ajazz Mouse: https://rustup.rs" >&2
  exit 1
fi
cargo build --release --locked

install -Dm755 target/release/ajazz-mouse "$bin"
mkdir -p "$data/applications"
sed "s|^Exec=.*|Exec=$bin|" "data/$id.desktop" > "$data/applications/$id.desktop"
install -Dm644 "data/$id.metainfo.xml" "$data/metainfo/$id.metainfo.xml"
for size in 128 256 512; do
  install -Dm644 "data/icons/hicolor/${size}x$size/apps/$id.png" "$data/icons/hicolor/${size}x$size/apps/$id.png"
done
refresh_icons

# Without the rule only root can talk to the mouse. A rule from a package or an older install also works.
if ! ls /etc/udev/rules.d/70-ajazz-*.rules /usr/lib/udev/rules.d/70-ajazz-mouse.rules >/dev/null 2>&1; then
  as_root sh -c "install -m 644 '$here/data/70-ajazz-mouse.rules' '$rule' \
    && udevadm control --reload-rules && udevadm trigger --subsystem-match=hidraw --action=change"
fi
echo "Done: Ajazz Mouse is in your app menu."
