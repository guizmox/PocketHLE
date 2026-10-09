PocketHLE — GPS Windows + Android

Extraire ce ZIP à la racine du dépôt PocketHLE, en remplaçant les fichiers.
Aucun script BAT/CMD fourni.

Commandes CMD :
  powershell -NoProfile -Command "Get-Content 'patch-files.txt' | ForEach-Object { (Get-Item -LiteralPath $_).LastWriteTime = Get-Date }"
  cargo build --release -p pocket-desktop

Activer « GPS / host location (GPS1) » dans Emulator options avant le lancement.
Windows : activer la localisation et l'accès des applications de bureau dans les
paramètres Windows. La demande de permission WinRT est initiée depuis le thread
UI ; l'acquisition reste asynchrone et ne bloque pas l'ARM. La précision dépend
du service de localisation du PC, aucun récepteur GPS n'est simulé artificiellement.
Android : activer l'option GPS et accorder la localisation précise au lancement.
Les abonnements sont suspendus en arrière-plan et fermés à la fin du jeu.

Importer tools/gpstest/dist/PocketHLE-GPSTEST.zip comme jeu Gizmondo.
Le test attend 30 secondes pour une position. GPSTEST_RESULT PASS valide les API ;
GPS_POSITION AVAILABLE confirme séparément la réception d'une position.
Le rapport et la structure brute sont dans Flash Disk (GPSTEST.TXT/GPSTEST.BIN).
COLORS_POSITION_ELIGIBLE YES confirme validité + précision <100 m. Colors exige
également un horodatage récent. Un service Windows peu précis peut donc être
fonctionnel tout en restant insuffisant pour le jeu.

Fonctions livrées : GPS1 / position native, format binaire SDK 180 octets, unités,
date UTC, absence de fix, permissions, partage, duplication et fermeture VFS.
La validation native est adaptée au bit FixValidated ; les compteurs de satellites
restent à zéro. L'altitude ellipsoïdale n'est pas présentée comme altitude MSL.
Les écritures geofence, commandes SiRF/APM et IOCTL de version non documenté
renvoient explicitement ERROR_NOT_SUPPORTED (50). Pas de notifications GNS,
pas de correction de l'horloge du PC et pas de localisation de fond Android.

Validation : 437 tests Rust passent. GPSTEST exécuté sur CPU ARM/Unicorn avec
fournisseur déterministe : PASS, position et éligibilité Colors confirmées.
Routines ARM GPS réelles de Colors (constructeur, lecture, destruction) exécutées :
coordonnées attendues et aucun handle VFS restant. 100 cycles de fermeture avec
duplication inter-processus vérifient la destruction de la capture au dernier handle.
Desktop Linux, bindings natifs Windows et JNI Android vérifiés par cargo check.
Pas de mesure sur matériel GPS Windows/Android ; APK Kotlin/Android non compilé
ici. Le test de Colors concerne son chemin GPS, pas une partie complète du jeu.
Aucune instrumentation par frame ni modification de l'EXE Colors livré.
