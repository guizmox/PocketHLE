PocketHLE - One-time public Android signing setup
================================================
1. Apply this patch to the repository. On your Windows PC, extract it first and
   double-click tools/setup-android-signing.bat (keep its PS1 beside it).
2. If Java keytool is missing, install Microsoft OpenJDK 17 or newer:
   https://learn.microsoft.com/java/openjdk/download
   Select the installer options for JAVA_HOME / PATH, reopen the BAT afterwards.
   Android Studio's bundled Java is detected too. Python is NOT required.
3. The tool creates ONE permanent release key at:
   %USERPROFILE%\.pockethle\android-signing\pockethle-release.jks
   It generates the password and alias for you. No password is sent anywhere.
   The four .txt files beside the key contain the values for GitHub secrets.
4. BACK UP THIS WHOLE FOLDER to a private safe location outside Git.
   Anyone with these files can sign as you. Losing this key prevents normal
   updates of installations signed with it. Never commit or publish these files.
5. Open https://github.com/guizmox/PocketHLE/settings/secrets/actions
   The script guides you through the four names below. For each one click
   New repository secret (or update the existing value), use the exact name,
   paste the clipboard VALUE into Secret, and save before continuing:
     POCKETHLE_KEYSTORE_BASE64
     POCKETHLE_KEYSTORE_PASSWORD
     POCKETHLE_KEY_ALIAS
     POCKETHLE_KEY_PASSWORD
   It does not upload secrets itself and cannot verify your GitHub settings.
6. Commit/push the patched scripts. Rerun CI on that commit, then replace the
   public Android release ZIP with the signed artifact. Do not reuse the old
   unsigned ZIP. The APK filename must not contain "unsigned".

Future releases automatically use the same key from GitHub secrets.
No need to rerun setup for each build. A complete existing signing folder is
reused; an incomplete folder causes an error, never automatic key replacement.
The script uses Windows account permissions to restrict the secrets directory.
If interrupted after key creation, keep the folder for recovery; do not delete
or regenerate a key that has already signed a published release.

Your personal WSL build remains debug by default and keeps its existing
~/.android/debug.keystore. This script does not access or modify that key.

Migration: APKs previously signed with a DIFFERENT CI debug key cannot update
in place. Back up the library first, then uninstall/reinstall and restore the
ZIP. Once this permanent key is configured, later releases can update normally.
Public push/manual CI builds now fail if signing secrets are absent. PR build
checks can still produce explicitly unsigned APKs; never publish those files.

Validation: shell guards tested locally without native compilation. Windows
interactive setup requires a Windows/JDK test; it was not executed on this host.
