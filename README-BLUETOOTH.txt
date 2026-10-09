PocketHLE — Bluetooth Classic / RFCOMM — 2026-10-09

Installation
============
Extraire à la racine du dépôt PocketHLE, en remplaçant les fichiers.
Fichiers complets, avec les corrections de la précédente livraison API générales.
Les DLL originales et les fichiers temporaires de validation ne sont pas inclus.

powershell -NoProfile -Command "Get-Content 'patch-files.txt' | ForEach-Object { (Get-Item -LiteralPath $_).LastWriteTime = Get-Date }"
cargo build --release -p pocket-desktop

Activation
==========
Dans Emulator options, activer "Bluetooth hardware (Classic / RFCOMM)" puis Save.
Le réglage est désactivé par défaut et s'applique au prochain lancement de jeu.
Allumer la radio Bluetooth dans Windows ; PocketHLE ne force pas son état global.
Appairer les appareils depuis les paramètres Bluetooth de leur OS.
Le message BT_MSG active/désactive le service émulé sous réserve de ce réglage.
Les autres jeux ne lancent pas de recherche ni de connexion Bluetooth.

Android : même réglage dans Settings. Les permissions Bluetooth / localisation
nécessaires sont demandées avant le lancement d'un jeu avec ce réglage activé.
Allumer la radio et appairer les appareils depuis Android. Une permission refusée
reste une vraie erreur ; aucune connexion n'est simulée pour masquer ce refus.

Test matériel Windows, indépendamment du jeu
============================================
Le build produit aussi target\release\pockethle-bt-test.exe.
Cet outil n'est jamais lancé automatiquement par le GUI.

Sur le PC serveur :
target\release\pockethle-bt-test.exe server

Sur le second PC, pendant que le serveur attend :
target\release\pockethle-bt-test.exe scan
target\release\pockethle-bt-test.exe client AA:BB:CC:DD:EE:FF

Remplacer AA:BB:CC:DD:EE:FF par l'adresse du PC serveur affichée par scan.
Le délai d'attente est de 60 secondes. Relancer le serveur si le délai expire.
PASS signifie connexion RFCOMM réelle + ping/pong dans les deux sens.
scan seul vérifie la recherche matérielle, pas l'échange de données.
Deux instances sur un seul PC ne suffisent pas : RFCOMM ne fournit pas un
loopback radio. Ce diagnostic est un exécutable natif, pas un test ARM invité.

Une fois ce test passé, vérifier le multijoueur du jeu entre deux PocketHLE,
en créant la partie sur l'un puis en rejoignant depuis l'autre. Sur Android,
la validation doit se faire avec un APK reconstruit et un vrai second appareil.

Parcours SDK pris en charge
==========================
BT_MSG, WSAStartup/WSACleanup, gethostname, WSALookupServiceBeginW/NextW/End,
RegisterDevice/DeregisterDevice pour btd.dll et COM1..COM9, ouverture COMn:,
SetCommMask/GetCommMask/WaitCommEvent pour EV_RXCHAR et annulation par masque zéro,
ReadFile/WriteFile synchrones. Les variantes sans suffixe W des fonctions de
recherche sont également reconnues. WS2 est accessible par LoadLibrary/GetProcAddress
avec les ordinaux relevés dans la DLL Gizmondo fournie.

Les handles COM se dupliquent et se transfèrent entre processus avec la connexion
et les permissions conservées. Désinscrire un périphérique annule ses opérations
en attente ; une nouvelle inscription COM4 fonctionne même si le jeu n'a pas fermé
ses anciens handles. Les écritures partielles reprennent à leur offset sans
dupliquer ni tronquer le paquet. Les buffers invités sont contrôlés avant les E/S.

Windows : Winsock AF_BTH non bloquant, recherche Bluetooth et annonce SDP.
Android : BluetoothSocket sécurisé, threads hôtes et files RX/TX bornées.
Les deux transports utilisent le même UUID de service par canal invité ; un GUID
explicite fourni par le programme est respecté.

Limites et validation
====================
415 tests logiciels passent : 145 kernel, 234 winceapi, 36 bibliothèque.
Le parcours SDK est testé via le dispatcher et un transport contrôlé : tailles
WSADATA/WSAQUERYSET, pointeurs ARM, découverte, attente, échanges, erreurs, masque,
duplication, annulation, réinscription et reprise d'écriture partielle.
Le frontend desktop est vérifié avec Unicorn et audio. Les modules Rust Windows
et JNI Android sont vérifiés avec les types de leurs API natives.
La vérification desktop Linux désactive temporairement la vidéo statique faute de
FFmpeg local et utilise xdg-portal ; ces adaptations ne sont pas livrées.

IMPORTANT : aucune liaison entre radios physiques n'a été testée ici.
Le code Kotlin et l'APK Android complet n'ont pas été compilés dans cet
environnement (SDK Android / dépendances de construction indisponibles).
Ces vérifications ne prouvent donc pas encore la compatibilité multijoueur réelle.

Ce patch n'implémente pas les 79 exports Winsock et 83 exports BTD dans leur
intégralité. Restent hors périmètre : sockets IP et sockets RFCOMM directement
ouverts par l'invité via Winsock, APIs bas niveau HCI/L2CAP/SDP du pilote BTD,
recherches de services ou avec filtres, E/S COM overlapped, MTU/quotas personnalisés,
contrôle UART/modem/DCB. REMOTE_DCB et KEEP_DCD sont acceptés pour le chemin SDK,
mais ne fournissent pas de contrôle modem/DCB. Sans transport disponible,
WSAStartup continue d'échouer réellement ; recv ne fabrique jamais un EOF.

L'interopérabilité avec une Gizmondo physique n'est pas garantie : le SDK emploie
un canal physique fixe tandis que les hôtes PocketHLE partagent un service UUID
dont le système alloue le canal RFCOMM. Le premier essai doit porter sur deux
hôtes PocketHLE. Windows recherche sur le premier adaptateur radio disponible.

Aucune instrumentation temporaire ajoutée. Les traces et l'audio ordinaires
conservent leur fonctionnement précédent.

Références techniques
=====================
SDK Gizmondo fourni : Examples/Bluetooth/Bluetooth.cpp ; exports ws2.dll/btd.dll.
https://learn.microsoft.com/en-us/windows/win32/bluetooth/bluetooth-and-wsaqueryset-for-set-service
https://learn.microsoft.com/en-us/windows/win32/bluetooth/bluetooth-and-bind
https://developer.android.com/develop/connectivity/bluetooth/connect-bluetooth-devices
https://developer.android.com/develop/connectivity/bluetooth/bt-permissions
