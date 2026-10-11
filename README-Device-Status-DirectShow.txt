PocketHLE — statut appareil et miniature DirectShow
Base : ab4d9df69886815334dc3097c1102f34a82fef95.
Patch cumulatif : inclut la livraison précédente des compléments API du SDK.

Android : lecture du mode avion, état SIM et radio cellulaire. Les bits du
GT_STATUS exposent mode avion et radio ; ce message ne possède pas de bit SIM.
Le snapshot SIM est disponible côté noyau, sans nouvelles API TAPI/RIL inventées.
Permission téléphone/localisation approximative pour le statut radio ; refus
accepté, état inconnu. Aucun identifiant SIM ou téléphone lu, aucun abonnement.
Cache une seconde, référence au contexte application. ActiveSync déconnecté.
Windows : simulation SIM prête, radio arrêtée, mode avion désactivé,
ActiveSync déconnecté. Internet reste indépendant de ces états.

DirectShow : invalide le cache de composition après le dessin GLES du menu.
La miniature est recomposée même lorsque le décodeur n'a pas encore produit
une nouvelle frame. Test utilisant le rectangle du launcher joint :
x=204, y=40, largeur=101, hauteur=82, sur plusieurs swaps.
Pas de journalisation temporaire ajoutée.

Validation : 186 tests noyau + 280 tests API avec FFmpeg statique et -D warnings,
contrôle JNI,
harness Kotlin (9 scénarios et 100 lectures). Validation physique Android et
Windows restante ; pas de compilation APK complète dans cet environnement.

Application : extraire à la racine du dépôt en remplaçant les fichiers.
patch-files.txt permet de rafraîchir les dates avant compilation.
Inclut les quatre fixtures DLL RAMTEST et un WMV synthétique, pas de jeu.
