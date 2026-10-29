#!/usr/bin/env bash
# Run from any working directory. Build the user-facing CI artifact, no native build.
set -euo pipefail
repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
output_dir=${1:-"$repo_root/dist/android-backup"}
mkdir -p "$output_dir"
output_dir=$(cd -- "$output_dir" && pwd)
archive="$output_dir/PocketHLE-Android-ADB-Backup.zip"
files=(backup-android-adb.bat backup-android-adb.ps1 backup-android-adb.sh
       backup-android-adb.py README-android-adb-backup.txt)
for file in "${files[@]}"; do
    [[ -f "$repo_root/tools/$file" ]] || { echo "Missing: tools/$file" >&2; exit 1; }
done
bash -n "$repo_root/tools/backup-android-adb.sh"
python3 - "$repo_root/tools/backup-android-adb.py" <<'PY'
import pathlib, sys
p = pathlib.Path(sys.argv[1])
compile(p.read_text(encoding="utf-8"), str(p), "exec")
PY
rm -f -- "$archive"
(cd "$repo_root/tools" && zip -q "$archive" "${files[@]}")
unzip -tq "$archive"
echo "Backup tools artifact: $archive"
