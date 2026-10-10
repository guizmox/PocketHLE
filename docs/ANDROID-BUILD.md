# PocketHLE — mise à jour Android cumulative

Ce patch s’applique directement à `PocketHLE(3).zip`. Il comprend la première base
Android et les changements demandés ensuite. Il contient exclusivement les fichiers
ajoutés/modifiés, listés dans `patch-files.txt`, sans binaires ni sauvegardes.
Extraire à la racine des sources, en conservant les dossiers et en remplaçant les
fichiers existants. Les correctifs Colors/CRT/GPS déjà dans ces sources sont conservés.

## Interface et réglages

- Bibliothèque en portrait : cartes SD neutres, noms des jeux, onglets Gizmondo/Pocket PC.
  Chaque carte possède un menu ⋮ : jouer, renommer, réglages du jeu, supprimer.
  Renommer conserve l’identifiant, les dossiers et les sauvegardes.
- Menu ⋮ principal → Settings : Emulator Settings, Affichage et contrôles,
  Gizmondo options, Clavier et manettes. Tous les réglages sont hors du jeu.
- Jeu en paysage plein écran : image centrale, boutons uniquement, un bouton Quitter.
  Stop/Rewind/Forward/Play à gauche, D-pad à droite, épaules L/R au-dessus.
  Les cinq fonctions sont Home, Volume, Brightness, Geofence, Power : mêmes
  symboles que la skin PC, VK F1/F2/F3/F4/F11. Power n’est pas la touche F5 du guest.
  Le clavier PC par défaut peut utiliser la touche hôte F5 pour produire VK_F11.
- Affichage Gizmondo 320×240 (4:3), sans étirement ; les commandes restent hors
  de l’image. Pocket PC conserve sa définition et sa rotation configurées.
- Auto, ×1/×2/×3/×4 : multiples des pixels natifs, limités à l’espace disponible
  pour que l’image entière reste visible. Ce réglage n’augmente pas la définition
  du moteur émulé. Le rendu garde le rapport d’image.
- Filtres : reconstruction, SMAA, SMAA doux, xBRZ, bicubique, Lanczos, bilinéaire,
  nearest. Même shader PC, SMAA de référence en trois passes, xbrz-rs 0.1.0
  en prétraitement ×3. GLES 3 est requis ; aucun filtre de remplacement maquillé.
- F10 d’un clavier physique : capture PNG des vrais pixels affichés après filtre
  et rotation, sans interface/boutons. Fichiers dans `library/screenshots/`.
- Gizmondo options : GPRS/data ; serveur Colors (hôte, IP ou origine HTTP(S)),
  ID joueur ; GPS réel ou position fixe ; Bluetooth ; caméra. Les options prennent
  effet au prochain lancement. Pas de consentement GPS réel nécessaire en position
  fixe ; une demande distincte peut rester nécessaire pour le Bluetooth ancien.
  Latitude signée −90..90, longitude −180..180 ; pas de satellites inventés.
- Le domaine fixe de Colors est redirigé par le pont WinINet existant. Exemples :
  `192.168.1.10:8080`, `nas.local:8080`, `http://192.168.1.10:8080`.
  Ne pas utiliser `localhost` pour un NAS : sur le téléphone il désigne le téléphone.
  Aucun changement du serveur NAS n’est nécessaire pour cette mise à jour Android.
- Sauvegardes Gizmondo : `library/flash/`, monté à `\Flash Disk\` comme sur PC.
  Configuration et registre de l’identité Colors restent persistants.

## Compilation depuis Windows : WSL2 / Ubuntu

Utiliser un SDK/NDK **Linux dans WSL2**, ainsi que Java et Rust Linux. Ne pas mélanger
le NDK Windows avec cargo Linux. L’installation unique nécessite Internet ; les
scripts téléchargent la version fixe de Gradle et FFmpeg si absents, puis vérifient
leurs empreintes. FFmpeg est lié statiquement : aucun ffmpeg.exe/dll à copier.

À la racine des sources, pour remettre les dates à jour depuis CMD :

```cmd
powershell -NoProfile -Command "Get-Content 'patch-files.txt' | ForEach-Object { (Get-Item -LiteralPath $_).LastWriteTime = Get-Date }"
wsl
```

Dans WSL2, se placer dans la racine des sources (par exemple
`cd /mnt/c/Users/TON_NOM/Documents/PocketHLE`). Pour des builds plus rapides,
une copie dans le système de fichiers Linux de WSL2 est préférable.

Prérequis à installer une fois :

```bash
sudo apt-get update
sudo apt-get install -y build-essential cmake curl unzip python3 openjdk-17-jdk
```

Installer Rust avec rustup si nécessaire : https://rustup.rs/.
Installer les command-line tools Android **Linux** depuis
https://developer.android.com/studio#command-line-tools-only, puis placer leur
contenu sous `$HOME/Android/Sdk/cmdline-tools/latest/` (le répertoire doit contenir
`bin/sdkmanager` directement).

```bash
export JAVA_HOME=/usr/lib/jvm/java-17-openjdk-amd64
export ANDROID_HOME="$HOME/Android/Sdk"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/28.2.13676358"
export PATH="$JAVA_HOME/bin:$ANDROID_HOME/cmdline-tools/latest/bin:$ANDROID_HOME/platform-tools:$PATH"
sdkmanager --licenses
sdkmanager 'platform-tools' 'platforms;android-35' 'build-tools;35.0.0' 'ndk;28.2.13676358'
rustup toolchain install 1.90.0 --profile minimal
rustup override set 1.90.0
cargo install cargo-ndk --version 3.5.4 --locked
```

Puis, pour chaque compilation :

```bash
export JAVA_HOME=/usr/lib/jvm/java-17-openjdk-amd64
export ANDROID_HOME="$HOME/Android/Sdk"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/28.2.13676358"
export PATH="$JAVA_HOME/bin:$ANDROID_HOME/platform-tools:$PATH"
bash tools/build-android-native.sh 4
bash tools/build-android-apk.sh
```

Le premier script compile FFmpeg et le pont Rust pour **arm64-v8a** et
**armeabi-v7a**. Le second compile un APK debug installable, vérifie l’alignement
16 Kio et la signature. Il utilise AGP 8.7.3, Gradle 8.10.2, Kotlin 2.1.21,
SDK 35, NDK r28c ; Android minimum 7.0 (API 24), targetSdk 34 conservé.
`4` est le nombre de tâches parallèles de FFmpeg, ajustable.

APK produit :

```text
frontends/pocket-android/app/build/outputs/apk/debug/app-debug.apk
```

Si un pont natif manque ou si son ABI/alignement/JIT est invalide, la compilation
s’arrête au lieu de produire un APK inutilisable. Sans Unicorn, le lancement
échoue explicitement ; il ne bascule pas vers un faux moteur de jeu.

Avec un téléphone accessible à adb dans cet environnement :

```bash
adb install -r frontends/pocket-android/app/build/outputs/apk/debug/app-debug.apk
adb logcat -s PocketHLE
```

Sous Windows on peut utiliser l’adb Windows pour installer l’APK copié depuis WSL2.
Le rapport des API manquantes est également enregistré dans le dossier de la
bibliothèque (`pockethle-unimplemented.log`), option réglable dans Emulator Settings.

Le workflow `.github/workflows/android.yml` permet aussi une compilation manuelle
sur GitHub Actions après intégration du patch. Aucun workflow n’a été lancé ici.

## Vérifications faites et limites

- Compilation/type-check des **17 fichiers Kotlin** avec les dépendances AndroidX,
  Android 14 et le R généré par AAPT2 ; ressources compilées et liées par AAPT2.
- 240 combinaisons de dimensions, rotation et zoom : coins, rapport d’image,
  rejet des touches dans les bandes et correspondance avec les pixels natifs.
- Préservation JSON des réglages PC, position GPS fixe, filtres, zoom et codes des boutons.
- 28 rendus OpenGL ES sous Mesa : sept modes GPU dans les quatre rotations ;
  compilation des shaders, FBO/lookup textures des trois passes SMAA, coins RGBA.
  xBRZ utilise la bibliothèque Rust PC, contrôlée lors du type-check JNI.
- Type-check Rust du pont JNI **sur Linux sans fonctionnalités natives**,
  38 tests `pocket-library` passés, syntaxe XML/TOML/Python/Bash vérifiée.
- Tests logiciels des périphériques : 4 tests GPS du moteur et 1 test ABI GPS,
  6 tests caméra du moteur et 6 tests ABI caméra, 3 tests Bluetooth série passés.
  Ils vérifient simulation et contrats invités, pas les capteurs physiques Android.
- L’archive FFmpeg 8.0.1 officielle a été téléchargée et son SHA-256 vérifié.

**Non exécutés ici** : cross-build des deux `.so` Android avec le NDK,
compilation complète Gradle/D8/signature APK, lancement sur téléphone,
Turf Wars Android et capteurs/radio physiques. Les vérifications ci-dessus ne
remplacent pas ces essais. Les scripts font les contrôles de packaging au build.
Le pont caméra/GPS/Bluetooth natif Android existant est conservé ; la couche
Winsock RFCOMM propre à Windows et un CLR/.NET CF général ne sont pas portés.

FFmpeg est compilé sans composants GPL/nonfree, selon ses conditions LGPL 2.1+.
xBRZ est GPL-3.0-only comme sur PC ; textes de licence joints aux assets.
Les shaders et tables SMAA conservent leur licence MIT. Pour distribuer un APK
publiquement, fournir les sources et les éléments de reconstruction/relinking requis
par ces licences ; ce patch est une livraison de sources.
