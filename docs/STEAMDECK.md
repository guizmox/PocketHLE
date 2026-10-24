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
| Menu — maintien de 650 ms | Arrêter le jeu et revenir au catalogue |
| View — appui court | Basculer le plein écran |
| View — maintien de 650 ms | Luminosité Gizmondo ; aucune touche sur PocketPC |

### PocketPC / WinCE

| Bouton Deck | Fonction |
|---|---|
| Croix / stick gauche | Directions |
| R2 | Action / Entrée |
| L2 | P1 (VK_APP1) |
| R3 — clic du stick droit | P2 (VK_APP2) |
| R1 | P3 (VK_APP3) |
| L3 — clic du stick gauche | P4 (VK_APP4) |
| L1 | Échap |
| A / B / X / Y | A / B / C / Tab (Ctrl / Espace / Maj / Tab) |

Les quatre boutons P1–P4 sont ceux annoncés par GAPI. Un jeu peut limiter son
écran de configuration à ces boutons et aux directions ; cela ne signifie pas
qu’il accepte aussi toutes les touches du clavier. Action / Entrée est distinct
de P1. Le clic d’un stick est indépendant de ses déplacements.

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
