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

| Bouton Deck | Fonction |
|---|---|
| Croix / stick gauche | Directions Gizmondo |
| A / B / X / Y | Play / Forward / Rewind / Stop |
| L1 | Plein écran |
| R1 | Home Gizmondo |
| L2 / R2 | Gizmondo L / R |
| Select / View — appui court | Filtre d’upscaling suivant, en boucle |
| Select / View — maintien de 650 ms | Arrêter le jeu et revenir au catalogue |
| Start / Menu — appui court | Volume Gizmondo (Piano 2) |
| Start / Menu — maintien de 650 ms | Luminosité Gizmondo (Piano 3) |

Le nom du filtre apparaît brièvement dans le jeu. Le dernier filtre choisi
est enregistré dans les réglages Deck. Start envoie les touches **Volume** ou **Luminosité de la Gizmondo** selon
la durée de l’appui ; il ne règle pas le volume ou la luminosité système de
SteamOS. L’appui long déclenche seulement Luminosité, sans ouvrir Volume avant.

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
