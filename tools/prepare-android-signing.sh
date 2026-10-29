#!/usr/bin/env bash
set -euo pipefail
# No secrets in PRs/forks: build an explicitly unsigned release, never a
# randomly signed debug APK which cannot update yesterday's installation.
android_signing_count=0
for android_signing_var in POCKETHLE_KEYSTORE_BASE64 POCKETHLE_KEYSTORE_PASSWORD POCKETHLE_KEY_ALIAS POCKETHLE_KEY_PASSWORD; do
    if [[ -n "${!android_signing_var:-}" ]]; then
        android_signing_count=$((android_signing_count + 1))
    fi
done
if [[ "$android_signing_count" == 0 ]]; then
    if [[ "${GITHUB_EVENT_NAME:-}" == pull_request ]]; then
        echo 'Pull request without secrets: unsigned APK for build checks only; not for public installation.'
        exit 0
    fi
    echo 'Public Android builds require all four POCKETHLE signing secrets. Run tools/setup-android-signing.bat once and configure repository Actions secrets. No unsigned public APK will be published.' >&2
    exit 1
fi
if [[ "$android_signing_count" != 4 ]]; then
    echo 'Configure all four POCKETHLE signing secrets.' >&2
    exit 1
fi
: "${RUNNER_TEMP:?}"
: "${GITHUB_ENV:?}"
umask 077
android_signing_path="$RUNNER_TEMP/pockethle-signing.jks"
printf '%s' "$POCKETHLE_KEYSTORE_BASE64" | base64 --decode > "$android_signing_path"
test -s "$android_signing_path"
printf 'POCKETHLE_KEYSTORE=%s\n' "$android_signing_path" >> "$GITHUB_ENV"
