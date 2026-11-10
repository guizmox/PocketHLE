# Historical validation records

These records preserve their original dates, environments, test counts and
limits. They are evidence for the recorded revisions, not current build recipes
or certification of PocketHLE 0.4.6. Some original reports remain in their supplied
language. Superseded commands and delivery ZIP references are retained only as
historical context; use [Build and release](../BUILD.md), the
[HARDWARE DIAG manual](../../tools/hardware-diag/README.md) and
[Android graphics diagnostics](../ANDROID-GRAPHICS-DIAGNOSTICS.md) for current work.

<a id="storage-memory-validation-2026-10-19"></a>
## Storage/memory validation — 2026-10-19

### VFS / RAM: couverture et validation — 19 octobre 2026

Ces diagnostics sont des tests de non-régression, pas une certification de
Windows CE ou de la Gizmondo. Un PASS ne garantit ni l'absence de corruption
arbitraire dans un jeu, ni l'absence de fuite GPU/audio côté hôte.

#### Ajouts automatiques

| Domaine | Scénario et critères vérifiés |
|---|---|
| VFSTEST v4 | 130 contrôles attendus : les 108 précédents + 22. Fichier binaire de 12 291 octets, tous les octets comparés après lecture puis réouverture ; taille des éléments stdio, EOF, flush, modification traversant 4 Kio, origine fseek invalide sans mouvement, erreurs après fermeture, refus d'écriture sur flux en lecture seule, nom Unicode, opérations de longueur zéro, trou de fichier rempli de zéros. |
| RAMTEST v9 | 145 contrôles attendus : les 136 précédents + 9. Tailles 1/7/8/4095/4096/4097/8191/8193, vérification complète des zéros et des données, libérations intercalées, allocations progressives de 1 Mio jusqu'à échec, données antérieures préservées, restitution RAM/espace virtuel, remise à zéro après décommit/recommit. |
| Modèle VFS Rust | 1 024 opérations déterministes sur chacun des stockages Flash et RAM. Lecture, écriture, déplacement, troncature comparés à un modèle indépendant en mémoire ; taille et curseur contrôlés à chaque étape, contenu après fermeture/réouverture. Graine fixe pour reproduire un échec. |
| Fragmentation Rust | 4 096 étapes d'allocations/libérations de tailles mixtes, absence de chevauchement des blocs vivants, restitution exacte et grande allocation finale prouvant la fusion des espaces libres. |
| Échec de commit Rust | Budget physique limité : commit partiel, refus d'une extension excessive sans changement des pages engagées ni du compteur, libération puis nouvelle allocation. |
| Concurrence Rust | Quatre threads hôtes, chacun avec un VFS et un alias du même fichier. 512 enregistrements de 64 octets ; aucune perte, duplication ou écriture déchirée ; fermeture du handle initial pendant que les alias vivent. Ne prétend pas explorer tous les ordonnancements. |
| Arrêt forcé Rust | Sous-processus écrivain arrêté de force après flush, handle encore ouvert. Le parent vérifie les 16 387 octets écrits ; la destruction normale du VFS du fils n'intervient pas. |
| Erreurs hôtes Linux | `/dev/full` provoque une véritable erreur d'écriture. Une limite de taille de fichier appliquée à un sous-processus provoque une véritable écriture partielle puis un échec ; position, taille, contenu et quota ne doivent compter que les octets effectivement écrits. |
| Échec de flush simulé | Empoisonnement volontaire du verrou du backend : flush doit échouer et ne pas altérer les données déjà écrites. C'est une injection d'erreur du backend, pas la reproduction d'un flush disque différé. |
| Confinement Unix | Un lien symbolique vers un répertoire externe ne doit permettre ni lecture, ni création, ni suppression hors montage. |
| API stdio Rust | Appels réels des handlers fwrite/fread/fseek/fflush/fclose avec mémoire invitée ; intégrité complète, lecture après réouverture, EOF et erreurs sur flux fermé. |

Le test d'origine `fseek` invalide a révélé un défaut produit : toute origine
inconnue était interprétée comme SEEK_SET. Le handler refuse désormais cette
valeur, sans modifier le curseur. Aucune tolérance ajoutée aux assertions RAM.

#### Ce qui a effectivement été exécuté ici

Linux : 229 tests kernel et 410 tests winceapi réussis, zéro échec ; quatre
tests winceapi restent ignorés par leur configuration existante. Les journaux
sont dans `verification/storage-memory-20261019/`. Le kernel a été relancé
après les deux derniers ajouts ; les modifications ultérieures de ce fichier
ne concernent que la documentation. Les tests stdio ont été exécutés après
correction de fseek ; l'échec attendu avant correction est également conservé.

Le test de fermeture forcée, celui d'écriture partielle et `/dev/full` ont
réellement tourné. Le contrôle rustfmt des fichiers modifiés, la syntaxe C des
diagnostics et la syntaxe Python des scripts ont été vérifiés.

Les versions ARM v4/v9 n'ont PAS été exécutées dans cette session : Clang/LLD
ARM et les fixtures reconstruites ne sont pas disponibles. Le contrôle C avec
le compilateur hôte ne remplace pas une compilation ARM ni l'exécution Unicorn.
Le nombre 130/145 est le résultat attendu à confirmer lors de cette exécution.
Pas de build release complet, pas de lancement GUI Windows/Android ici.

#### Relancer les vérifications

Depuis la racine du projet, sous PowerShell ou un terminal Linux :

```powershell
python tools/run-storage-memory-tests.py
```

Pour une passe complète puis neuf répétitions des nouveaux tests ciblés :

```powershell
python tools/run-storage-memory-tests.py --repeat 10
```

Pour reconstruire les diagnostics et exécuter les tests ARM d'intégration,
installer Clang et LLD dans le PATH ainsi que les prérequis du backend Unicorn :

```powershell
python tools/run-storage-memory-tests.py --guest
```

Ce mode reconstruit RAMTEST, VFSTEST et TIMETEST, car le fichier d'intégration
Rust inclut leurs fixtures à la compilation. Il exécute aussi les variantes
de sortie de processus RAM et tous les rapports secondaires. Un prérequis
manquant ou un échec arrête le script ; aucun mode n'est silencieusement ignoré.
Les journaux locaux sont écrits dans `verification/storage-memory-local/`.
Le runner et son aide ont été contrôlés ; sa séquence complète doit être
lancée dans un environnement possédant les prérequis ci-dessus.

#### Vérifications restant nécessaires sous Windows et Android

1. Reconstruire/importer les nouveaux ZIP de diagnostic, les lancer deux fois
   en fermant complètement PocketHLE entre les lancements. Vérifier les lignes
   finales exactes, les rapports DLLTEST/DEPTEST/PROCTEST/ORPHANTEST et le compteur
   de persistance. Un ancien ZIP de diagnostic conserve les anciens tests.
2. Refaire les tests sur les emplacements réellement utilisés : stockage interne,
   carte SD si disponible, chemins avec espaces et accents. Conserver l'OS,
   la version du build, le système de fichiers et les journaux pour comparaison.
3. Pour un disque plein/une erreur au flush, utiliser exclusivement un volume
   ou une image de test de petite capacité et des fichiers jetables. Comparer
   l'ancien contenu, la taille, le curseur, les retours API et la réouverture
   après l'erreur. Les tests Linux ne prouvent pas les mêmes erreurs sous NTFS
   ou dans l'accès au stockage Android.
4. Répéter lancement/arrêt des diagnostics et un parcours de jeu pendant une
   session prolongée. Relever la mémoire du processus hôte entre cycles après
   stabilisation (Gestionnaire des tâches/Process Explorer sous Windows,
   `adb shell dumpsys meminfo <package>` sous Android). Chercher une croissance
   répétée, en distinguant caches, mémoire invitée, mémoire native et GPU.
   La seule égalité des compteurs RAM invités ne suffit pas.

#### Limites explicites

- Le kill du processus après flush ne simule ni une coupure électrique ni la
  persistance matérielle. Pas de garantie de sauvegarde atomique : il faudrait
  tester le protocole temporaire/renommage propre à chaque application.
- Le test de flush empoisonné ne couvre pas une panne physique ou une erreur
  différée de chaque couche de tampon. Il faudra une injection au niveau d'un
  backend d'E/S abstrait si une telle couche est introduite.
- Aucun fuzzing illimité, aucune exploration exhaustive des courses concurrentes.
- Pas de test général de toutes les combinaisons VirtualProtect/VirtualQuery,
  de tous les formats PE, de toutes les API WinCE ou de tous les chemins Unicode.
- Les tests ne valident pas l'usure réelle d'une mémoire Flash.


<a id="original-hardware-diagnostic-validation"></a>
## Original hardware diagnostic validation

> Historical validation of the original suites, before HARDWARE DIAG consolidation.
> This is not a full execution report for the new integrated application.

### Diagnostic update validation

Base: guizmox/PocketHLE `4123cff2c98c10592bc44a3ac4d55fcbc7e924a5`.
Clang 18.1.8 / LLVM LLD, Rust 1.90.0; tests use locked dependencies and warnings denied.

- Kernel: 188 tests passed.
- WinCE APIs: 288 passed; three native presentation tests initially ignored, then all three passed explicitly.
- GLES: 136 passed; 20 native-driver tests passed explicitly. The timing benchmark remains excluded.
- Library: 50 passed.
- Unicorn: five RAM/VFS/timing integrations plus five hardware integrations passed.
- RAMTEST: 136 checks passed, including DLL/DEP/PROC/ORPHAN side reports.
- VFSTEST: 108 checks passed on both runs, marker counter advanced 1 to 2.
- TIMETEST: 44 checks passed; silent audio completion and software EGL timing.
- Controlled CAM/GPS/VIB/NET providers returned live counts to zero; BT startup and inquiry state cleaned up.
- All eight ARM/WinCE diagnostics rebuilt; ZIP CRC and PE machine/subsystem checked.
- Full workspace rustfmt and git diff whitespace checks passed.

The one-shot runner was exercised with `--guest --native-gpu` and returned PASS.
Native rendering used Mesa surfaceless/software-host GL/GLES, not a physical GPU.
Hardware providers were controlled test adapters, not Windows/Android devices.
No complete Windows GUI/APK build or physical radio/camera/GPS/TLS/audio test was
performed here. No RSS/driver/JIT leak certification is claimed.

Detected corrections: closed/unknown DeviceIoControl handles set LastError 6;
DirectShow test fixtures share a test-only graph gate to avoid parallel resets.
No production resource monitor, game-specific instrumentation or unrelated
PocketPC upstream-sync change is included.


## Android packaging validation

Historical packaged-build evidence (these earlier counts do not certify the
current checkout; rerun the commands above after changes):

- Kotlin/Java compilation, AAPT2 resources and D8 packaging with AndroidX.
- 240 viewport/native-size/rotation/scale combinations: exact integer scaling,
  Auto's largest fitting scale, inverse corner mapping and letterbox rejection.
- Configuration JSON preservation, fixed GPS, filter/scale settings and the
  fifteen SDK control codes, including Power as VK_F11.
- 28 Mesa OpenGL ES renders covering seven GPU modes in four rotations:
  shader compilation, reference three-pass SMAA FBO/lookups and RGBA corners.
  xBRZ uses the desktop Rust implementation.
- JNI Rust type checks, 38 pocket-library tests, XML/TOML/Python/Bash validation.
- Device contract tests: four kernel GPS and one GPS ABI test, six kernel camera
  and six camera ABI tests, and three Bluetooth serial tests. These exercise
  simulation and guest contracts, not physical Android sensors/radios.
- Official FFmpeg 8.0.1 download with SHA-256 verification.
- Release cross-builds of both native ABIs with NDK r28c, Unicorn and static
  FFmpeg; ELF, 16 KiB alignment and JIT symbol checks.
- Two targeted COREDLL `_wcsrev` tests.
- Gradle debug APK build, APK v2 signature verification and
  `zipalign -c -P 16 4`. Both packaged native ELF libraries were checked.

**Not covered by that evidence:** launch on a physical phone, Android Turf
Wars, or physical GPS/camera/Bluetooth tests. Build checks do not replace those
tests. The existing Android camera/GPS/Bluetooth bridge is retained. Windows-specific
Winsock RFCOMM and a general CLR/.NET CF runtime are not ported.

### Q3CE validation (2026-11-07)

Linux x86-64, native Release build, 640x480, GPU backend using Mesa llvmpipe.
The same recorded Q3DM1 scene renders 580 timedemo frames per pass. Runs are
sequential (reference, cache, cache, reference), without tracing or background
builds. The replay fixes camera/server snapshots; it does not benchmark live
server/bot simulation.

| Code-hook lookup | First pass | Second pass | Engine frame rate |
| --- | ---: | ---: | ---: |
| Reference | 48.9 s | 49.1 s | 11.8–11.9 fps |
| Cached | 31.1 s | 25.0 s | 18.6–23.2 fps |

These are local measurements, not a prediction for Windows, physical GPUs,
Android or other workloads. Both variants hit the same existing UIARM undefined
instruction after completing/logging the timedemo, on return to the interface;
this optimization does not suppress that separate failure.

Native validation: 714 API tests, 341 kernel tests and 49 CPU tests pass. The API
suite excludes exactly one independently reproduced baseline dialog-geometry
failure (client height 85 versus expected 84); 13 existing tests remain ignored.
Dedicated C tests also pass against the reference and cached ARM/AArch64/MIPS
engines, including callback mutation, nested execution/deferred hook deletion,
Thumb IT, instruction counting and 64-bit address collisions. The original
native permissions, self-modifying code and shared-memory tests pass as well.

A final rebuild of the delivered source (including the nested-deletion guard)
repeats the 580 frames in 25.8 s, 22.5 fps. Formatting and diff checks pass.

## Optimization origins

* Fast memory access: `edbf1ac727203a2ddb7294d7912067a69b3ee1d7`.
* Extended memory translation cache: `96e8ea670976eb98f687c40249042312ce89439a`.
* Responsive thread scheduling: `7c9d309de695be46becb99d76250e1476dd663d5`.
* Extended jump cache: `c98e0cfeb71a469dc92b3b3bd677945dd9d5a8e7`.

## Colors investigation records

<a id="colors-colors-save-compatibility-turf-wars-and-performance"></a>
## Colors: save compatibility, Turf Wars and performance

<a id="colors-startup-action"></a>
### Startup action

Windows/Linux/Steam Deck and Android show **Unlock Turf Wars + Tag Editor**
in the existing Colors startup dialog. On Deck, X invokes the same action.
It runs before starting the game, off the UI thread; Start and repeated actions
are disabled until it finishes. It never runs automatically on later launches.

- Decrypt and validate `config.cfg` and an existing `Save/SaveFile.isv` before
  changing either file.
- Set `TurfWarUnlocked=1` and `TagEditorUnlocked=1`, preserving all other config settings.
- Leave the entire ISV byte-identical, regardless of Money or ServerMoney.
- Never contact the server, grant money, or change registration identity.
- Repeated clicks keep both unlock flags enabled, including when Turf Wars was already unlocked by an earlier version. They preserve pending solo
  gains, previous online purchases, and the synchronization checkpoint.
- If no save exists, do not invent one. Preserve missions and all other settings.

Only a changed original config gets a unique `config.cfg-before-turf-*.bak`.
Config replacement is atomic. Unknown/corrupt formats, ambiguous case variants
and symlinks are rejected before writing. Use `GameEntry::gizmondo_flash_dir`.

<a id="colors-save-validation"></a>
### Save validation

The supplied config decodes to `TurfWarUnlocked=0`; the supplied ISV contains
Money=525 and ServerMoney=525. The codec was reconstructed from the original ARM
byte transform at 0xe9600/0xe96dc, including feedback and rotating key state.
The startup unlock itself requires no executable patch.

Local diagnostic entry points invoked the supplied executable's original config
reader/writer and ISV parser/serializer through PocketHLE. With the generic
absolute-write fix applied, the game opens the original Flash paths for rb/wb.
The original ISV round-trips to exactly its original plaintext. Earlier funding-policy diagnostics also verified that the ARM parser/serializer
accepts changed balances. The current startup action leaves the ISV untouched. Config read/write retains
all other settings; the host unlock changes only TurfWarUnlocked and TagEditorUnlocked to 1.
Original uploaded files remain untouched; all runs use copies.

Rust tests cover codec/key rotations, lower/equal/higher balances, a higher server
balance, idempotency, backups, case-insensitive paths, missing saves, malformed
structures and failure before changing config when the save cannot be decoded.
Desktop/Deck and JNI are type-checked under Linux. The changed Kotlin files pass
Kotlin 2.1.21 syntax parsing; this is not a complete Android APK build or device UI
validation. CI should rebuild both native libraries and the APK together.

<a id="colors-performance-investigation"></a>
### Performance investigation

The complete supplied archive uses the same executable as the standalone upload.
The measured scene is the tutorial dialogue with The Bum, after loading central.
Runs use the real ARM CPU, native offscreen GLES on headless Linux/Mesa, silent
host audio, a 240x320 CLI framebuffer and sparse captures. The graphical
launcher normally configures this Gizmondo title at 320x240; these timings
are therefore comparisons of the same diagnostic setup, not final device FPS. These are
not physical Windows, Deck or Android performance measurements.

API timings and guest execution intervals were first collected separately,
grouped by presentation-counter phase. The first two groups include loading;
subsequent groups contain the 3D scene. A return address seen after a CPU slice is
only an attribution clue: it is not a function's execution time. Candidate
functions were then measured directly at their actual entry and return addresses,
without stopping or modifying their instructions.

| Routine in the supplied ARM image | Direct observations |
| --- | --- |
| 0x912b8, return 0x91e04 | 1780 calls, 3.984 seconds cumulative, approximately 2.24 ms/call. A later scene window adds 900 calls and 2.036 seconds, approximately 20 ms per image across about 100 images. |
| 0x26080, return 0x26b00 | Same 1780 calls, 0.134 seconds cumulative, approximately 0.075 ms/call. A frequent return address here did not imply an equally costly function. |
| 0x99544 and nested 0x994b8 | Respectively about 2.68 million and 2.62 million calls during loading; counters stop increasing in the subsequent 3D scene. They do not explain sustained tutorial-frame cost. |

The 0x912b8 disassembly iterates model tracks/transforms and interpolates fixed
point values, including signed 64-bit divisions. **Model-pose/animation update is
an inferred role, not a recovered original symbol name.** Its repeated direct
cost makes it a concrete optimization target. Rendering the scene on the GPU does
not move this guest routine to the GPU. Changing animation update rate or skipping
this function would alter the game; neither is part of this patch.

Loading also issues over a million intercepted runtime calls (many temporary
string allocations/deletions and numeric conversions). The heavily repeated
loading routines are a separate optimization target. Direct hooks on millions
of calls noticeably perturb loading, so their instrumented elapsed times are
not used as uninstrumented loading benchmarks. Timings for other missions and
crowded city scenes may differ; no whole-game speedup is claimed.

All instrumentation, alternate entry points and extra mounts were local probes.
No profiler, permanent game hooks, changed game resource or frame-rate override
is included in production. The generic optimization also handles four pure COREDLL helpers inside the CPU
run loop: `__rt_sdiv64by64`, `__rt_udiv64by64`, `__rt_srem64by64` and
`__rt_urem64by64`. It removes the kernel/dispatcher round-trip, not the guest
animation calculation. Unicorn stops before reading/writing registers and resumes
through the thunk's padded ARM `bx lr`; signed/unsigned results, zero divisors and
signed overflow match the existing handlers exactly. Other backends retain the
normal dispatcher. Older divisor-first helper variants are unchanged.

The original CPU slice deadline remains fixed across all helper calls, including
an API-free arithmetic loop. Static named/ordinal imports, dynamic exports and
native DLL imports use the same installer; unmapping removes the helper hook.
Tests compare 8,324 inputs with independent wide-integer arithmetic, preserve
registers/flags/FPSCR, return to Thumb, and verify scheduler preemption, remapping,
LastError and neighbouring API dispatch. This is shared emulator code for PC,
Deck and Android; no Colors address or executable fingerprint is used.

An experimental callback implementation was discarded after a loading crash in
one repeat. The delivered implementation only changes registers while Unicorn is
stopped. Repeat-run results and limits are included with the patch. No full-game,
physical-device or universal FPS improvement is asserted.

<a id="colors-optimization-measurements"></a>
### Optimization measurements

Alternating old-dispatcher / CPU-local runs of the same diagnostic binary use
minimal identical entry/return probes, the same inputs, and no concurrent build
in measured runs. Between presentation counters 175 and 275, each run executes
exactly 900 calls of 0x912b8. Old path: 1980.196 and 2039.207 ms; CPU-local path:
1884.634 and 2005.930 ms. Mean elapsed time drops about 3.2%, but run-to-run
variation overlaps: this is a modest local result, not a robust whole-game FPS
claim. Both optimized repeats finish the loading/tutorial sequence without a
crash. A separate 320x240 run is a stability check only, not an A/B benchmark.

An isolated diagnostic ARM loop performs 100,000 signed 64-bit divisions through
the original Colors IAT ordinal 2000. Alternating process wall times including
startup: old path 213.6/224.5 ms, CPU-local 118.7/124.3 ms (about 44.5% less).
This isolates call overhead; it is not representative of game-wide performance.
No diagnostic executable or copyrighted game data is distributed in the patch.

<a id="colors-startup-presentation-and-turf-wars-transfer-error"></a>
### Startup presentation and Turf Wars transfer error

The startup form shows only the unlock button. Its result and balance policy
appear in an acknowledged popup after the action, with no backup notice or
launcher welcome/status text in the form. Backups continue internally. Deck A/B
dismiss the popup without starting a game on the same input.

With the supplied complete game, Fake GPS Paris (48.8584, 2.2945), a local server
at GizmondoColorsTurfWarsServer 0d3765a, actual ARM and GPU rendering, the map GIF
arrives in HTTP 200 but the game displays a localized transfer error (English translation: "Transfer error. Try again.")
The original coordinate parser at 0xf2704 returns false if either turf coordinate
is <= 0 (0xf279c..0xf27b0). The signed server grid around Eiffel Tower includes
negative X and zero Y, so the metadata callback rejects it after image decoding.
This is a server protocol mismatch; the hardening commit did not introduce the
signed coordinate scheme. No emulator bypass or game patch is appropriate.

The separate server patch uses a fixed +1,000,000 bias for both client turf IDs,
with inverse conversion on incoming turf commands. GPS degrees, signed SQLite
keys, map projection, ownership and grid origin stay unchanged. Map tile rendering
also converts the wire IDs back to signed storage coordinates before fetching.
The same game now displays the interactive Paris map without the transfer error.
The Linux diagnostic uses a temporary loopback HTTP backend and deterministic
real raster tile bytes; Windows WinHTTP, Android HttpURLConnection and the live
public deployment are not exercised by this run. Restart/redeploy the server
with its patch; updating PocketHLE alone does not repair the server response.

<a id="colors-independent-solo-and-online-wallets"></a>
### Independent solo and online wallets

The previous local funding policy is removed. Unlock never writes Money or
ServerMoney. Existing saves previously granted 75,000 are preserved, not reset.
A server starting-money floor of 75,000 (configurable) applies only to new online
registrations. Existing online accounts are not topped up on reconnect.

`pocket-core::colors_wallet` adapts only the supplied retail executable, SHA-256
`065c6f62e793210e7889cab087495a42a84c77b7b41899410156017cb43d7a92`.
Unknown versions remain unchanged. The checked patch changes only the loaded
image; the EXE on disk is never modified. It uses original encrypted save routines
to preserve solo Money and checkpoint ServerMoney to solo Money. The online
amount stays in the online user object. Only positive subsequent solo earnings
are added to the online amount; solo spending does not debit the online wallet.
No graphics paths are changed.

Original ARM functions: balance reader 0x17cd8, save writer 0x17ef4,
response parser 0xf1a9c, registration builder 0xe6880, sync builder 0xe66d4.
The mode-2 registration/sync ACK also checkpoints the local balance. Ordinary
refreshes retain the original game's checkpoint timing. This does not add
transaction IDs or exactly-once recovery after network failures to the legacy
protocol, nor authenticate offline earnings.

`tools/verify_separate_wallets.py` in the companion server patch executes the
original ARM request builders, parser, encrypted ISV I/O and balance arithmetic,
using the exact bytes exported by the Rust compatibility test and a real local
HTTP/SQLite backend. Verified: solo 525 -> online 75,000; online purchase ->45,000
while solo stays 525; solo gain +1,000 ->online 46,000 and solo 1,525; reconnect
adds nothing. Solo spending, forced refresh, saturation, existing accounts,
and the supplied save are also covered. This is an isolated native protocol
fixture, not a full gameplay or physical-device test. Both app and server updates
are required. Starting funds must be changed in deployed custom rules too.

<a id="colors-explicit-eiffel-tower-gps-refresh"></a>
### Explicit Eiffel Tower GPS refresh

In Colors only, with Fake GPS and Near Eiffel Tower selected, desktop/Deck F12
chooses a new simulated position within 800 m of the tower. The hint appears
only under that selected startup option. Holding F12 does not repeat.
Android Geofence (VK 0x73) adds the same action on the press edge, retaining
both original guest key-down and key-up events. Other games and manual/host
GPS modes retain their original input behavior. Manual coordinates are not
changed or saved. No server request is added: the game receives the new fix
on its next GPS read and performs its usual map refresh.

Desktop/Deck Geofence also supplements normal guest input with the Eiffel refresh,
for keyboard, controller and onscreen keys routed as VK 0x73. F12 remains available.
Latitude/longitude and coordinate guidance appear only in manual mode; Android
also hides Fill GPS controls outside enabled manual mode and cancels pending fills.
Stored manual values survive mode switching.

## Superseded NVIDIA UI probe handout

This handout described a temporary WGL probe. It is not an installation guide
and its capture launcher is not part of the current workflow.

> Extraire à la racine, recompiler :
> cargo build --release -p pocket-desktop
> Fermer PocketHLE puis lancer capture-nvidia-ui.cmd.
> Même scène : repos 5 s, touche maintenue 5 s, repos 5 s, souris en mouvement
> 5 s, clic immobile 5 s. Quitter normalement et envoyer le log complet.
> Indiquer également si le stutter subsiste.
>
> Marqueur attendu : revision=wgl-core-check-v3.
> Suppression du chemin wglGetCurrentReadDCARB et de sa recherche d'adresse.
> Les contrôles natifs du contexte et de la fenêtre de dessin restent actifs.
> Glutin WGL ne supporte pas une surface de lecture séparée; PocketHLE ne
> l'utilise pas. Pas de cache de l'état courant ni changement des entrées.
> Les colonnes historiques wgl_proc_address, wgl_read_dc et wgl_read_window
> restent présentes à zéro pour comparer les logs.
>
> Sondes désactivées au lancement normal. Pas de modification FullGPU ou
> des replis CPU. Trois tests passent, dont EGL natif. Helper WGL contrôlé
> en metadata avec stubs; test matériel Windows/NVIDIA à effectuer.
