PocketHLE — caméra Gizmondo CAM1 — Windows + Android

Ce ZIP contient les fichiers sources complets à remplacer à la racine du dépôt.
Il conserve aussi les corrections Bluetooth et manette SDL2 du patch précédent.
Aucun script BAT/CMD n'est fourni. Ne remplace pas le Cargo.toml racine, ni
Cargo.lock, ni votre configuration de compilation locale.

WINDOWS — depuis votre console CMD, après extraction dans le dépôt :

cd /d C:\Users\gtristant\source\repos\PocketHLE
powershell -NoProfile -Command "Get-Content 'patch-files.txt' | ForEach-Object { (Get-Item -LiteralPath $_).LastWriteTime = Get-Date }"
set CMAKE_POLICY_VERSION_MINIMUM=3.5
cargo build --release -p pocket-desktop

Activer Settings > Emulator options > Camera hardware (CAM1), puis lancer
un jeu ou le diagnostic. La case est désactivée par défaut ; elle autorise
l'accès, et le jeu déclenche réellement la capture via CAM_START.
Windows utilise la première webcam. Dans les paramètres Confidentialité de
Windows, autoriser la caméra aux applications de bureau si nécessaire.

TEST ARM — importer dans la librairie :
tools/camtest/dist/PocketHLE-CAMTEST.zip

Lancer CAMTEST après activation de la caméra. Il écrit dans le dossier flash
de la librairie : camtest.txt, camtest-preview.bmp, camtest-capture.i420.
Attendre CAMTEST_RESULT PASS et ouvrir le BMP pour vérifier l'image réelle.
Relancer une deuxième fois pour vérifier la libération/réouverture de la caméra.
Voir tools/camtest/README.md pour les détails et la conversion I420 avec FFmpeg.
Ces fichiers de test remplacent uniquement leurs propres sorties CAMTEST.

ANDROID — sources intégrées ; permissions CAMERA, pause/reprise et fermeture.
Activer Camera hardware dans les settings. L'autorisation CAMERA est demandée
au lancement. La caméra arrière est préférée, sinon la première disponible.
Avec votre installation NDK/JDK/Gradle habituelle, depuis la racine du dépôt :

cargo ndk -t arm64-v8a -t armeabi-v7a -o frontends/pocket-android/app/src/main/jniLibs build --release -p pocket-android-jni
cd frontends\pocket-android
gradle assembleDebug

L'APK est dans app\build\outputs\apk\debug. Utiliser votre commande Gradle
habituelle si Gradle n'est pas installé dans PATH. L'environnement de livraison
n'a pas de SDK/JDK Android opérationnel : la construction de cet APK n'a pas
été validée ici.

CONTRAT IMPLEMENTE
CAM1 : SETFORMAT, GETFORMAT, START, STOP, PREVIEW, CAPTURE.
Aperçu RGB565 top-down, multiples de 8 jusqu'à 640x480, maximum 20 ips.
Capture 640x480 I420 (Y/U/V). Buffers guest validés avant consommation.
Délais conservés entre reprises du scheduler ; duplication et partages
respectés ; dernier CloseHandle/STOP libère la capture matérielle.
Pas de capture simulée quand la caméra est absente ou interdite.
Les IOCTL 2107/2108/2109 non documentés et l'overlapped retournent unsupported.
La caméra PocketPC via DirectShow et les contrôles capteur non documentés
ne font pas partie de ce pilote CAM1.

VALIDATION
456 tests logiciels réussis (151 kernel, 37 library, 245 WinCE API, 23 desktop).
CAMTEST ARM exécuté avec Unicorn et caméra synthétique : 15 contrôles PASS.
BMP vérifié : 320x240, couleurs RGB565 correctes, pas d'inversion verticale.
Backend Windows Media Foundation et pont JNI/integration Rust type-checkés.
Pour la vérification Linux de l'intégration JNI, seul le logger Android non
disponible a été omis dans un fichier temporaire, sans changer les sources
livrées. Desktop vérifié avec Unicorn/CPAL ; FFmpeg statique non reconstruit.
Pas de test de webcam réelle ni de caméra Android ici, ni de build APK complet.
I420 est le choix Y/U/V standard ; le SDK indique YUV420 sans documenter
explicitement l'ordre des plans chroma sur le matériel Gizmondo.

Pas de nouvelles traces de diagnostic activées dans les jeux. CAMTEST est un
outil volontaire à importer/lancer ; il ne s'exécute pas automatiquement.
