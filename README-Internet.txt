PocketHLE – Internet Windows + Android

Extraire ce ZIP à la racine du dépôt en remplaçant les fichiers.
Les fichiers complets conservent les modifications RAM/VFS, affichage, audio,
caméra, Bluetooth et GPS de la version de travail actuelle.

Dans CMD, à la racine du dépôt :
powershell -NoProfile -Command "Get-Content 'patch-files.txt' | ForEach-Object { (Get-Item -LiteralPath $_).LastWriteTime = Get-Date }"
cargo build --release -p pocket-desktop

Windows : aucun réglage à activer, le jeu utilise la connexion du PC.
Android : reconstruire l'APK ET la bibliothèque native Rust selon le processus
habituel du projet. INTERNET est déclaré ; HTTP historique est autorisé ; HTTPS
vérifie les certificats normalement. Aucun nouveau crate réseau tiers.

Scope : neuf API WinINet HTTP/HTTPS réclamées par Colors, GET/POST, en-têtes,
statuts, flux binaires, cookies de session, redirections, proxy, GetLastError,
attente non bloquante pour le CPU invité, annulation et fermeture en cascade.
Ce patch ne généralise pas encore les sockets TCP/UDP Winsock hors Bluetooth,
ni FTP, callbacks WinINet asynchrones, ou toutes les options WinINet Windows CE.
Les huit autres imports CRT/date de Colors identifiés ne font pas partie de
cette livraison. Les anciens serveurs Gizmondo ne sont pas recréés ici.

Test matériel : importer tools/nettest/dist/PocketHLE-NETTEST.zip, puis lancer.
Attendre les deux requêtes HTTP et HTTPS. NETTEST.TXT est écrit dans le Flash Disk
propre au test. Les erreurs indiquent GetLastError en hexadécimal.
Ensuite tester Colors et communiquer son log/API log si besoin.

Validation ici : 442 tests Rust réussis ; ARM NETTEST loopback réussi (GET,
POST, 256 Ko/réponse, ABI UTF-16, EOF, handles). Contrôles compilation desktop,
JNI et signatures WinHTTP réussis. Pas d'exécution native Windows/TLS ni de
compilation APK Android dans cet environnement ; validation appareil requise.
Aucune instrumentation par frame ajoutée.
