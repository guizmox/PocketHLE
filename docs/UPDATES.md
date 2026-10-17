# Startup release checks

The desktop launcher (Windows/Linux and the dedicated Steam Deck edition) and
Android catalogue check the latest published stable release of
`guizmox/PocketHLE` once at startup. They use the public GitHub Releases API,
not Actions artifacts or commits. No GitHub account/token is required.

Checks run off the UI thread, with network timeouts and a 1 MiB response cap.
No release, invalid data, API rate limits and offline/TLS failures are silent.
HTTPS certificate validation remains enabled. Desktop supports environment
proxies and system certificate roots, retaining public roots for Proton hosts
whose Windows store is empty. No game/catalogue data is sent or modified.

Only a strictly newer semantic version is offered. Tags use `v0.3.3` or
`0.3.3`; drafts, prereleases and non-version tags are ignored. Build metadata
does not change precedence. Desktop compares the workspace Cargo version.
Android's generated `BuildConfig.UPDATE_VERSION` reads that same version from
the root Cargo.toml; its existing `versionName` test suffix is not the update
comparison baseline.

On desktop, the prompt waits until the catalogue is available, outside imports,
gameplay and other launch prompts. Download opens the system browser; Later
dismisses it for this launch. Enter/A downloads and Escape/B dismisses, including
on Steam Deck without a mouse. Android uses a dismissible catalogue Snackbar
with Download, deferred until the activity is resumed and imports are finished.
Activity recreation reuses the process result instead of starting more requests.

The matching release asset is selected when unique:

| Edition | Release asset |
| --- | --- |
| Windows | Windows ZIP, excluding Steam Deck |
| Steam Deck | Dedicated steamdeck/steam-deck ZIP |
| Linux | Linux ZIP or tar.gz |
| Android | APK |

Absent or ambiguous assets open that release's page. Download URLs must belong
to this repository's GitHub release downloads. There is no background package
download, automatic installation, EXE replacement or APK installation request.

## Publishing an update

1. Increase `[workspace.package].version` in root Cargo.toml and regenerate
   Cargo.lock. For Android distribution also increase `versionCode`; keep its
   displayed `versionName` appropriate for the release.
2. Build and validate the packages from that commit with CI.
3. Publish a stable GitHub Release with the matching tag, for example `v0.3.3`.
   Attach the Windows, Linux and Steam Deck packages and the Android APK as
   release assets. Actions artifacts alone do not trigger a release notification.

The checker does not publish releases or change repository settings. A stable
`v0.3.1` release is already present; applications whose comparison version is
`0.3.1` must not offer it as a newer update.

## Validation

Rust and Android share `crates/pocket-library/tests/data/releases.json`, covering
14 version and asset cases. Rust also tests platform selection and Linux package
formats. Android runs its JUnit cases in `tools/build-android-apk.sh` before APK
assembly; the normal CI already invokes that script. Full native packaging and
physical-device/browser interaction remain checks for the target builds.
