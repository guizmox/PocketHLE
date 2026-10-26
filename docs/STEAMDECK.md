# PocketHLE — édition Steam Deck

Cette édition est la version Windows dédiée à SteamOS via Proton.

## Télécharger le build CI

Dans GitHub Actions, ouvrir une exécution réussie du workflow **CI**, puis
télécharger l’artefact **PocketHLE-steamdeck-x86_64**. Il contient le ZIP
`PocketHLE-steamdeck-x86_64.zip`, avec `pockethle-steamdeck.exe` et ce guide.

Extraire le ZIP sur la Deck. Dans le raccourci Steam existant, choisir
`pockethle-steamdeck.exe` comme cible et garder Proton activé. Les options de
lancement peuvent rester vides : le profil SDL pour Proton est intégré.
Utiliser un modèle Steam Input de type **Manette / Gamepad**, qui transmet
les boutons Xbox. Les boutons arrière ne sont pas nécessaires.

## Commandes en jeu

Les boutons Menu (☰, à droite) et View (à gauche) servent à l’émulateur :

| Bouton Deck | Fonction |
|---|---|
| Menu — appui court | Filtre d’upscaling suivant |
| Menu — maintien de 650 ms | Démarrer / arrêter l’enregistrement vidéo |
| View — appui court | Basculer le plein écran |
| View — maintien de 650 ms | Luminosité Gizmondo ; sortie du jeu sur PocketPC |

### PocketPC / WinCE

| Bouton Deck | Fonction |
|---|---|
| Croix / stick gauche | Directions |
| L1 | P1 (VK_APP1) |
| R1 | P2 (VK_APP2) |
| L2 | P3 (VK_APP3) |
| R2 | P4 (VK_APP4) |
| L3 — clic du stick gauche | Échap |
| R3 — clic du stick droit | Action / Entrée |
| A / B / X / Y | A / B / C / Tab (Ctrl / Espace / Maj / Tab) |

Les quatre boutons P1–P4 sont ceux annoncés par GAPI. Un jeu peut limiter son
écran de configuration à ces boutons et aux directions ; cela ne signifie pas
qu’il accepte aussi toutes les touches du clavier. Action / Entrée est distinct
de P1. Le clic d’un stick est indépendant de ses déplacements. Si SDL les expose, les
boutons Steam (Guide/Mode) et Quick Access (Misc1) permettent aussi de sortir du
jeu PocketPC par appui long. SteamOS peut réserver ces boutons au système.

### Gizmondo

| Bouton Deck | Fonction |
|---|---|
| Croix / stick gauche | Directions |
| A / Y / B / X | Action / A / B / C |
| L1 | Volume Gizmondo (Piano 2) |
| R1 | Home Gizmondo (Piano 1) |
| L2 / R2 | Gizmondo L / R |

Les assignations Gizmondo personnalisées restent conservées, à l’exception de
L1 réservé au volume. Les commandes Gizmondo règlent la machine émulée, pas le
volume ou la luminosité de SteamOS.

## Catalogue

Croix ou stick gauche pour sélectionner une carte, **A** pour lancer,
**X** pour changer de catégorie, **Y** pour les options du jeu et **Start**
pour les réglages. **B** revient depuis les menus sans enregistrer les
modifications. Le catalogue défile pour garder la sélection visible.
Dans le menu de démarrage Colors : directions pour la langue, A pour lancer,
B pour annuler. L’import initial et la saisie de texte peuvent nécessiter le
trackpad ou un clavier.

## Réglages et mise à jour

L’édition Deck utilise `config-steamdeck.json` et la version standard utilise
`config.json`. Elles peuvent partager le catalogue d’une même bibliothèque.
Le préréglage Deck initial (Start = Home) est automatiquement remplacé par le
nouveau préréglage (Start = Volume) uniquement s’il correspond encore
exactement aux anciennes valeurs d’usine. Les assignations personnalisées sont
conservées. **Apply Steam Deck preset** permet de rétablir toutes les valeurs
Deck en un clic, si souhaité.

## Compiler sous Windows

```text
cargo build --release --locked -p pocket-desktop --features steam-deck --bin pockethle-steamdeck
```

Le fichier produit est `target/release/pockethle-steamdeck.exe` (ou
`target/cargo/release/pockethle-steamdeck.exe` dans la CI).
La compilation standard sans `steam-deck` garde ses commandes habituelles.

## Catalogue et captures

L1/L2 passent à l’onglet précédent et R1/R2 au suivant, en boucle : Gizmondo / PocketPC / Recordings. Le menu à trois points de chaque jeu contient Informations. Dans Recordings, A ouvre la capture sélectionnée avec le lecteur par défaut. Share apparaît uniquement si le système fournit un panneau de partage natif ; aucune liste de destinations fixe n’est proposée.

Un appui long sur Menu démarre la vidéo ; le bouton peut être relâché. Un nouvel appui long arrête la vidéo. Le témoin REC avec point rouge clignotant est visible uniquement sur l’écran local, après la capture du framebuffer.

Dans le catalogue, A lance le jeu sélectionné ; Y ouvre son menu à trois points. Haut/bas au stick ou à la croix parcourent Informations, Play, Rename, Settings et Remove ; A valide. B quitte un sous-menu et revient au menu du jeu, puis B ferme ce menu.

## Navigation des réglages à la manette (PC et Steam Deck)

Haut/bas parcourent les champs ; gauche/droite ajustent les curseurs. A actionne le contrôle sélectionné (bouton, checkbox ou liste déroulante). Dans une liste, haut/bas parcourent les choix, A valide et B ferme sans modifier le choix. B quitte ensuite les réglages. Le focus apparaît avec un contour bleu, et les pages de réglages défilent pour le garder visible.

A sur un champ de texte ouvre le clavier virtuel. Stick/croix choisissent les touches, A les saisit, Y bascule vers les symboles ; Shift change la casse. Apply valide le texte ; B annule la saisie. Les boîtes de fichiers externes restent celles du système d’exploitation.

Dans Recordings : A ouvre la capture sélectionnée ; Y ouvre le partage natif lorsqu’il est disponible.
