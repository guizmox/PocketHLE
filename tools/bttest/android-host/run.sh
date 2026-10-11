#!/usr/bin/env bash
set -euo pipefail
# Host JVM regression only. Requires JDK17, kotlinc, Android API jar, org.json jar.
: "${ANDROID_JAR:?Set ANDROID_JAR to an Android API 33+ framework jar}"
: "${JSON_JAR:?Set JSON_JAR to org.json:json:20240303}"
test_root="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$test_root/../../.." && pwd)"
test_out="$(mktemp -d)"
trap 'rm -rf "$test_out"' EXIT
kotlinc -classpath "$ANDROID_JAR:$JSON_JAR" -include-runtime -d "$test_out/test.jar" \
    "$repo_root/frontends/pocket-android/app/src/main/java/com/pockethle/app/RfcommHost.kt" \
    "$test_root/FakeBluetooth.kt" "$test_root/FakeHost.kt" "$test_root/Test.kt"
java -classpath "$test_out/test.jar:$JSON_JAR" com.pockethle.app.TestKt
