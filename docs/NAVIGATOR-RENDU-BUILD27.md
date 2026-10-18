# PocketHLE 0.3.3 — cumulatif rendu Navigator, build 27

## Base conservée

Archive utilisateur `crates(1).zip`, conservée intégralement dans cette livraison avec ses correctifs existants (Navigator/CoPilot, VFS, contrôles, fenêtres modales, chemin d'exécutable Gizmondo et SkyStriker). Aucun patch du binaire ARM ni contournement d'une écriture à l'adresse zéro.

L'archive reçue ne contenait pas Cargo.toml/Cargo.lock racine ni les fixtures DLL de RAMTEST. Métadonnées racine et outils RAMTEST complétés depuis guizmox/PocketHLE/main ; les DLL de test proviennent de tools/ramtest/dist/PocketHLE-RAMTEST.zip du dépôt. Les sources utilisateur ont priorité : seuls les fichiers indiqués comme modifiés dans CHANGEMENTS-BUILD27.txt ont été changés.

## Diagnostic et correctifs

Les appels ImageList_* manquants ne pouvaient pas fournir les images attendues par les contrôles. ImageList_Create renvoyé à zéro entraîne les ImageList_ReplaceIcon suivants avec un handle nul, exactement comme dans le journal fourni. LoadIconW renvoyait aussi un handle factice constant : implémenter seulement les listes aurait laissé cette autre rupture dans la chaîne de ressources.

- Images et listes possédées par le processus : dimensions réelles, indices, copies indépendantes, ajout/remplacement/suppression/destruction, compte et taille réels.
- Résolution des ressources bitmap/icône/curseur dans leur module propriétaire, y compris une DLL native. Décodage DIB, bitfields RGB565, masque AND, inversion monochrome, alpha 32 bits et PNG. Limites d'allocation explicites.
- LoadIconW, LoadImageW, LoadCursorW, DrawIcon/DrawIconEx et ImageList_* branchés sur les pixels réels ; handlers nommés partagés COREDLL/COMMCTRL. Synchronisation des DIB détenus par l'invité avant/après dessin.
- List views : image de LVITEM, association des listes, rendu, hauteur/hit testing cohérents et défilement de sélection ; images des boutons et contrôles statiques. La composition ne fabrique plus de frames supplémentaires en dessinant ces contrôles.
- GetFileTime retourne les métadonnées VFS disponibles ; CompareFileTime compare les valeurs FILETIME 64 bits. Dates indisponibles, notamment fichiers RAM, à zéro.
- SetClassLongW/GetClassLongW mémorisent les attributs par classe et respectent les sous-classages de fenêtres. RegisterClass/GetClassInfo reflètent ces valeurs.
- ScrollWindowEx/ScrollWindow copient les pixels avec recouvrement, tiennent compte du clipping, retournent la région découverte et son rectangle, gèrent le déplacement des enfants et l'invalidation. Effacement différé limité aux zones découvertes. Régions GDI rectangulaires associées.

## Limites explicites

Ce correctif répare des causes démontrables d'absence d'images ; il ne prouve pas que tous les assets et menus de Navigator sont désormais fidèles. L'archive reçue contient les sources, aucun CoPilot.exe, aucune DLL ni données cartographiques du jeu : Navigator n'a pas été lancé pendant cette reprise. Le crash ARM initial et l'arrivée en navigation ne sont donc pas revalidés ici.

Reste à vérifier sous Windows avec le jeu : démarrage, ressources chargées par DLL native, apparence des menus, navigation, et parcours complet avec boutons Gizmondo. Le test de sélection au clavier vérifie la cohérence du contrôle, pas toute la chaîne d'entrée console.

Portée partielle des API : LR_LOADFROMFILE non implémenté (échec explicite), pas d'overlays d'image list, pas de grille ListView Win32 complète, pas d'animation SW_SMOOTHSCROLL. Aucun ajout d'ordinal COMMCTRL deviné.

Voir VALIDATION-BUILD27.txt et verification/ pour les commandes réellement exécutées et leurs résultats.
