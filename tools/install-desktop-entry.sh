#!/usr/bin/env bash
# Install the native Linux/Deck launcher and icon for the current user.
set -euo pipefail
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
if [[ -f "$script_dir/pockethle.png" ]]; then
    icon_path="$script_dir/pockethle.png"
    default_executable="$script_dir/pockethle-gui"
else
    icon_path="$repo_root/frontends/pocket-desktop/assets/pockethle-256.png"
    default_executable="$repo_root/target/release/pockethle-gui"
fi
executable="$(realpath -- "${1:-$default_executable}")"
[[ -x "$executable" ]] || { echo "Executable not found: $executable" >&2; exit 1; }
data_root="${XDG_DATA_HOME:-$HOME/.local/share}"
mkdir -p "$data_root/applications" "$data_root/icons/hicolor/256x256/apps"
cp "$icon_path" "$data_root/icons/hicolor/256x256/apps/pockethle.png"
# Desktop Entry Exec quoting has its own escaping rules; paths can contain spaces.
escaped="${executable//\\/\\\\}"
escaped="${escaped//\"/\\\"}"
escaped="${escaped//\$/\\\$}"
escaped="${escaped//\`/\\\`}"
escaped="${escaped//%/%%}"
cat > "$data_root/applications/PocketHLE.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=PocketHLE
Comment=Gizmondo and PocketPC emulator
Exec="$escaped"
Icon=pockethle
Terminal=false
Categories=Game;Emulator;
StartupWMClass=PocketHLE
EOF
command -v update-desktop-database >/dev/null && update-desktop-database "$data_root/applications" || true
