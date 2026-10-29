#!/usr/bin/env bash
# Create a PocketHLE restore ZIP. Never uninstall or alter app data.
set -euo pipefail
script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
export PATH="$script_dir/platform-tools:$script_dir:$PATH"
trap 'echo "Backup did not finish. KEEP THE APP INSTALLED. Check the error above." >&2' ERR
finish() {
    result=$?
    [[ -z "${scratch:-}" ]] || rm -rf -- "$scratch"
    if [[ -t 0 ]]; then
        printf '\nPress any key to close this window...'
        IFS= read -r -s -n 1 _key || true
        printf '\n'
    fi
    exit "$result"
}
trap finish EXIT
output=${1:-"PocketHLE-backup-$(date +%Y%m%d-%H%M%S).zip"}
serial=${2:-}
cat <<'INTRO'
PocketHLE - Android library rescue backup
----------------------------------------
This saves your games, settings and saves to a ZIP on this computer.
It NEVER uninstalls PocketHLE or modifies its library.

Before starting:
  - Install Android Platform Tools (adb), zip, unzip and tar.
    Debian/Ubuntu: sudo apt install adb zip unzip tar
  - Enable USB debugging on the Android device and connect it by USB.
  - Unlock the device and accept its USB debugging authorization prompt.
  - Close any running game/import and leave enough disk space for TWO copies
    of your library (temporary files plus the finished ZIP).
  - If several devices are connected, provide the device serial as argument 2.


ADB download (official, Windows/Linux):
  https://developer.android.com/tools/releases/platform-tools
Extract the download, then put the platform-tools folder beside this script,
or add its full path to PATH. Android Studio is NOT required.

To enable ADB / USB debugging on the phone or handheld:
  1. Open Settings > About phone (or About device).
  2. Tap Build number seven times; enter your PIN if asked.
     Samsung: About phone > Software information > Build number.
  3. Open Settings > System > Developer options (location varies by device).
  4. Enable USB debugging. Bootloader unlocking / OEM unlocking is NOT needed.
  5. Connect a USB data cable, unlock the device, and accept Allow USB debugging.
  6. If ADB says unauthorized, check that authorization prompt on the device.
Official guide: https://developer.android.com/studio/debug/dev-options

Do NOT uninstall PocketHLE until this script reports "Verified backup".
INTRO
for command in adb zip unzip tar find grep; do
    command -v "$command" >/dev/null || { echo "Missing command: $command" >&2; exit 1; }
done
[[ ! -e "$output" ]] || { echo "Output already exists: $output" >&2; exit 1; }
output_dir=$(cd -- "$(dirname -- "$output")" && pwd)
output="$output_dir/$(basename -- "$output")"
scratch=$(mktemp -d "$output_dir/.pockethle-rescue.XXXXXXXX")

adb_cmd=(adb)
[[ -z "$serial" ]] || adb_cmd+=(-s "$serial")
fail() { echo "$* KEEP THE APP INSTALLED." >&2; exit 1; }
echo "[1/4] Checking the ADB connection..."
"${adb_cmd[@]}" get-state
echo "[2/4] Stopping PocketHLE and copying its library (large libraries may take a while)..."
"${adb_cmd[@]}" shell am force-stop com.pockethle.app
library="$scratch/library"
external=/sdcard/Android/data/com.pockethle.app/files/library
if ! "${adb_cmd[@]}" pull "$external" "$library" || [[ ! -f "$library/library.json" ]]; then
    echo "Direct access unavailable. Trying the installed debug app (run-as)..."
    rm -rf -- "$library"
    for source in "$external" /data/user/0/com.pockethle.app/files/library; do
        if ! "${adb_cmd[@]}" exec-out run-as com.pockethle.app tar -C "$source" -cf - . > "$scratch/rescue.tar"; then
            continue
        fi
        # Reject links, special files and paths outside the destination before extraction.
        tar -tf "$scratch/rescue.tar" > "$scratch/names" || continue
        tar -tvf "$scratch/rescue.tar" > "$scratch/types" || continue
        if grep -qvE '^[-d]' "$scratch/types"; then fail 'Unsafe TAR entry.'; fi
        while IFS= read -r name; do
            case "$name" in /*|*\\*|*:*|../*|*/../*|*/..) fail 'Unsafe TAR path.' ;; esac
        done < "$scratch/names"
        mkdir -p "$library"
        if tar -xf "$scratch/rescue.tar" -C "$library" --no-same-owner --no-same-permissions && [[ -f "$library/library.json" ]]; then
            break
        fi
        rm -rf -- "$library"
    done
fi
[[ -f "$library/library.json" ]] || fail 'Android refused access; no usable backup created.'
[[ -z "$(find "$library" ! -type d ! -type f -print -quit)" ]] || fail 'Links/special files are unsupported.'
echo "[3/4] Creating the restore ZIP..."
printf '%s\n' '{"format":"PocketHLE","version":1}' > "$library/__pockethle_backup__.json"
(cd "$library" && zip -q -r "$scratch/backup.zip" .)
echo "[4/4] Verifying every ZIP entry..."
unzip -tq "$scratch/backup.zip" || fail 'ZIP validation failed.'
unzip -Z1 "$scratch/backup.zip" | grep -qx 'library.json' || fail 'Missing catalogue.'
# Hard link publishes the verified ZIP atomically without overwriting an existing file.
ln -- "$scratch/backup.zip" "$output" || fail 'Cannot publish ZIP (destination already exists?).'
echo "Verified backup: $output"
echo 'Keep a separate copy of this ZIP before uninstalling anything.'
echo 'After installing the new APK, use PocketHLE: Restore library backup (ZIP).'
echo 'If ADB reports unauthorized, unlock the device and allow USB debugging.'

