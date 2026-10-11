PocketHLE — compléments API du SDK Gizmondo
Base : ab4d9df69886815334dc3097c1102f34a82fef95 (origin/main récupéré pour cette livraison).

Contenu
- GetDateFormatW/A et GetTimeFormatW/A : formats, dates réelles, tailles et erreurs.
- GT_GETSTATUS / GT_STATUS et requêtes GPRS via SendMessageW ou PostMessageW.
- Chargement réel des accélérateurs des ressources et WM_COMMAND synchrone.
- DirectShow : IMediaSeeking, IMediaPosition, IBasicVideo, position/visibilité vidéo,
  recherche FFmpeg réelle, audio seul, capture DIB et libération des objets.

Validation locale : 279 tests WinCE API avec FFmpeg statique + 184 tests noyau,
avec -D warnings ; contrôle de types JNI. Inclut 100 cycles COM et un WMV synthétique.
Les quatre DLLs RAMTEST incluses sont des fixtures générées depuis les sources
existantes du projet : elles permettent aux deux tests de chargement de fonctionner
sur un clone où ces fichiers ignorés par Git sont absents.
Pas de diagnostic temporaire ajouté. Archives FFmpeg et configuration inchangées.

Limites explicites
Les locales par défaut/en-US et fr-FR sont supportées ; l'horloge locale virtuelle
reste UTC. Calendriers alternatifs, suppression de champs avec un format horaire
personnalisé, automation COM IDispatch, captions BSTR, vitesses autres que 1,
segments et recherche sans vidage retournent une erreur explicite.
Le statut ne simule pas une SIM, le GSM, ActiveSync ou un mode avion.
Ce patch couvre les quatre blocs de l'audit SDK ; il ne prétend pas implémenter
l'intégralité de Windows CE/DirectShow. Validation physique Windows/Android restante.

Application : extraire le ZIP à la racine du dépôt, en remplaçant les fichiers.
Puis utiliser les commandes données dans la conversation. Aucun script .bat.
Le fichier patch-files.txt répertorie les fichiers livrés pour le rafraîchissement
des dates de modification avant compilation.
