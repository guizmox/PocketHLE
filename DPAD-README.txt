Patch complet D-pad + disposition validee + zone morte elargie.
Base GitHub: 8e964ab. Remplace les precedents patches D-pad.
Extraire a la racine du depot puis reconstruire APK avec le script habituel.
Rayon neutre: 31.5dp au lieu de 11.2dp, sur le D-pad de 140dp.
Le disque central montre le rayon reel. Les secteurs tactiles restent actifs
entre les branches dessinees, jusqu aux bords de la cible carree.
Glissement, diagonales, relachement et annulation inchanges, sans delai ajoute.
Compilation Kotlin et tests de 19600 positions et touch passes sur hote.
A verifier sur telephone: poser le pouce au centre puis deplacer vers les
branches; une direction doit commencer plus loin du centre qu auparavant.
Pas de modification Rust/JNI; reconstruction native inutile pour ce patch.
