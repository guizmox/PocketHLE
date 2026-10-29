PocketHLE - Android library rescue backup (no Python required)
============================================================

Purpose
-------
Save the installed library before replacing an APK with a different signing key.
The scripts NEVER uninstall PocketHLE, clear its data, or modify game saves.
They stop the running app so the copy is consistent. Do not launch it again
until the backup finishes. Keep the old installation if anything fails.

1. Install ADB
--------------
Download Android SDK Platform-Tools for your computer from Google's official page:
https://developer.android.com/tools/releases/platform-tools

Extract it. Put the entire platform-tools folder next to these scripts:
  backup-android-adb.bat
  backup-android-adb.ps1
  backup-android-adb.sh
  backup-android-adb.py
  platform-tools/adb.exe       (Windows)
  platform-tools/adb           (Linux)
Keep the other Platform-Tools files too (including DLLs on Windows).
Alternatively, add your existing Platform-Tools folder to PATH.
Android Studio, root, OEM unlocking and bootloader unlocking are NOT required.

Linux also requires Bash, zip, unzip and tar. Debian/Ubuntu:
  sudo apt install adb zip unzip tar
On Steam Deck, run the script from Desktop Mode in Konsole; the script does not
install system packages or disable the read-only system. Install/provide the
listed commands first if any are missing.

2. Enable USB debugging
----------------------
- Settings > About phone / About device > tap Build number seven times.
  On Samsung: About phone > Software information > Build number.
- Enter the screen-lock PIN if requested.
- Open Developer options (often Settings > System > Developer options).
- Enable USB debugging.
- Connect a USB DATA cable, unlock the device and approve Allow USB debugging.
Official instructions (menus vary by manufacturer):
https://developer.android.com/studio/debug/dev-options

3. Run ONE script
-----------------
Windows 10/11: extract this ZIP first, keep BAT and PS1 together, then double-click
backup-android-adb.bat. It uses the Windows PowerShell already included in Windows,
not Python. The window stays open at the end, including on error, until you press a key.
By default, the ZIP is saved in the current working directory.
For an explicit destination, open a terminal in the extracted tools folder:
  backup-android-adb.bat "C:\Users\YourName\Desktop\PocketHLE-backup.zip"

Linux, from the extracted tools folder (run in a terminal):
  bash backup-android-adb.sh "$HOME/PocketHLE-backup.zip"

Python alternative (Python 3.9+):
  python backup-android-adb.py PocketHLE-backup.zip
On Linux, use python3 if needed.

If several ADB devices are connected, get the serial with adb devices, then:
  backup-android-adb.bat PocketHLE-backup.zip DEVICE_SERIAL
  bash backup-android-adb.sh PocketHLE-backup.zip DEVICE_SERIAL
  python backup-android-adb.py PocketHLE-backup.zip --serial DEVICE_SERIAL
Use the full path to adb (or .\platform-tools\adb.exe on Windows) if not on PATH.

Leave enough space for TWO copies of the library on the destination drive.
Games are included, so large libraries can take a long time. Existing output
files are never overwritten; choose another filename for a new attempt.

The Bash/Python scripts also wait for a key when run in an interactive terminal.
Without an interactive terminal (automation), they exit normally without waiting.

4. Wait for "Verified backup"
-----------------------------
The scripts first try adb pull. If Android blocks access, they try run-as,
which requires the INSTALLED old APK to be debuggable. The Windows fallback
also needs tar.exe (included with recent Windows 10/11).
If both access methods fail, KEEP THE APP INSTALLED. Use its built-in ZIP
backup if available. These scripts cannot bypass Android's access restrictions.

The ZIP contains the library's games, settings, saves and Flash directories,
including separate alternative-build saves. It is not an Android system backup.
Linux and Python check ZIP CRCs; Windows reads back each archived file and
compares SHA256 against the copied source. No physical-device validation was
performed for this package; Windows execution still needs a real Windows test.

5. Replace the APK and restore
-----------------------------
Keep a second copy of the verified ZIP before uninstalling anything.
Only after successful backup, uninstall the old APK and install the new one.
Copy the ZIP to the Android device (for example, its Downloads folder).
Open PocketHLE and choose Restore library backup (ZIP), then select the ZIP.
Do not restore it with adb backup/adb restore: this is PocketHLE's own ZIP format.

Troubleshooting
---------------
- adb not found: check Platform-Tools placement or PATH.
- unauthorized: unlock the device and approve the USB debugging prompt.
- no device / offline: reconnect a data cable, try another port, check adb devices.
- Windows device not detected: install your manufacturer's ADB USB driver.
  https://developer.android.com/studio/run/oem-usb
- Linux permission denied: configure your distribution's Android USB/udev access.
- package not debuggable: run-as cannot read that APK's data; direct pull or the
  built-in PocketHLE backup must work before uninstalling.

GitHub Actions artifact
-----------------------
The main CI workflow publishes an independent artifact named:
  PocketHLE-Android-ADB-Backup
Open the workflow run > Artifacts and download it. GitHub may wrap the included
PocketHLE-Android-ADB-Backup.zip in an artifact ZIP; extract both layers if needed.
The scripts are packaged without waiting for Android/native compilation.
The helper tools/package-android-adb-backup.sh reproduces this ZIP locally.
