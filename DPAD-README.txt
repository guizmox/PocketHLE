Base GitHub: 8e964ab. Ce patch inclut le D-pad continu ET le nouveau placement.
Remplace le precedent patch D-pad; extraire a la racine du depot.
L/R: 124x132dp, marge haute 8dp. D-pad: marge basse 8dp.
Exit orange, espace 28dp apres Power; fonctions decalees a gauche.
Reconstruire l APK dans WSL: bash tools/build-android-apk.sh
Pas de changement Rust/JNI ni de reconstruction native necessaire.
Compilation Kotlin et controles hote de touch/input/geometrie passes.
Gameplay et APK sur telephone restent a verifier.
