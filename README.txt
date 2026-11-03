Extraire à la racine, recompiler :
cargo build --release -p pocket-desktop
Fermer PocketHLE puis lancer capture-nvidia-ui.cmd.
Même scène : repos 5 s, touche maintenue 5 s, repos 5 s, souris en mouvement
5 s, clic immobile 5 s. Quitter normalement et envoyer le log complet.
Indiquer également si le stutter subsiste.

Marqueur attendu : revision=wgl-core-check-v3.
Suppression du chemin wglGetCurrentReadDCARB et de sa recherche d'adresse.
Les contrôles natifs du contexte et de la fenêtre de dessin restent actifs.
Glutin WGL ne supporte pas une surface de lecture séparée; PocketHLE ne
l'utilise pas. Pas de cache de l'état courant ni changement des entrées.
Les colonnes historiques wgl_proc_address, wgl_read_dc et wgl_read_window
restent présentes à zéro pour comparer les logs.

Sondes désactivées au lancement normal. Pas de modification FullGPU ou
des replis CPU. Trois tests passent, dont EGL natif. Helper WGL contrôlé
en metadata avec stubs; test matériel Windows/NVIDIA à effectuer.
