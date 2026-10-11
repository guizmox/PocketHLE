# Android RFCOMM host regression

`run.sh` compiles the actual production `RfcommHost.kt` against fake radio
objects and runs it on JDK17. Set `ANDROID_JAR` (API33+) and `JSON_JAR`
(`org.json:json:20240303`), with `kotlinc` on PATH, then:

```sh
bash tools/bttest/android-host/run.sh
```

Checks 100 bidirectional exchanges with Chicane's channel-address style,
nonblocking connect/accept, MSG_PEEK, receive availability, send shutdown,
connection errors, invalid/stale handles, listener teardown with an unclaimed
peer, and complete socket/worker release. Doubles live only in this test folder;
they are never included in the Android application.

This does not exercise the radio, pairing UI, permissions or device discovery.
Physical verification: enable Bluetooth in PocketHLE and Android on two phones,
start Chicane on both, host on the first, accept Android's discoverability dialog,
then join/scan on the second. Accept pairing if Android requests it. Leave the
multiplayer menu, repeat with the host/client roles reversed, and exit both games.
Discoverability expires after five minutes; recreate the host if necessary.
Both APKs must include the updated JNI library as well as Kotlin code.

Android's public API chooses physical RFCOMM channels via UUID/SDP; PocketHLE
uses a stable UUID per logical guest channel when the guest supplies no GUID.
Windows also publishes/resolves this UUID, enabling PocketHLE Android/Windows
combinations. Original consoles using only physical channel numbers are outside
this mapping. Physical verification is required for all host/client combinations;
a Windows radio may require pairing in its system Bluetooth settings first.
