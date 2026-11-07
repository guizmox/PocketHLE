# Référence des affectations par défaut

Deck est la référence commune. Les valeurs sont contrôlées par les tests Rust et Android.
Les préréglages d'usine anciens sont migrés ; les tables personnalisées ne sont pas réécrites.
Les boutons réservés conservent la priorité du Deck.

## Clavier

| Touches | Commande |
|---|---|
| Z / S / Q / D | Haut / Bas / Gauche / Droite |
| Bas / Haut / Droite / Gauche | Action / A / B / C |
| 1 / 2 | Soft 1 / Soft 2 |
| T | Turbo en Pocket PC ; Brightness en Gizmondo (même VK_F3) |
| F1 / F2 / F3 / F4 / F5 | Piano Gizmondo Home / Volume / Brightness / Geofence / Power |
| O / L / K / M | Stylet Haut / Bas / Gauche / Droite en Pocket PC uniquement |
| F10 | Capture d'écran |
| F11 | Enregistrement sur desktop ; indisponible sur Android |
| F12 | Plein écran |

P1–P4 n'ont pas de touche clavier par défaut. Les alias piano restent dans la
configuration clavier partagée, suivant la référence Deck.

## Manette

| Contrôle | Pocket PC | Gizmondo |
|---|---|---|
| D-pad / stick gauche | Directions | Directions |
| South | A | Play / Action |
| North | Soft 1 (Tab) | Stop / A |
| East | B | Forward / B |
| West | C | Rewind / C |
| L1 / R1 | P1 / P2 | Volume / Home |
| L2 / R2 | P3 / P4 | Soft 1 / Soft 2 |
| L3 / R3 | Soft 2 (Esc) / Action | Libres |
| Stick droit | Stylet | Libre |
| View court | Plein écran | Plein écran |
| View maintenu 650 ms | Retour bibliothèque | Brightness |
| Menu court | Filtre suivant | Filtre suivant |
| Menu maintenu 650 ms | Enregistrement | Enregistrement |
| Guide maintenu, si livré par l'OS | Retour bibliothèque | Libre |

L'enregistrement est disponible sur desktop ; Android signale son indisponibilité.
Extra/Misc1 est une entrée desktop dans l'adaptateur actuel. Les autres contrôles
communs emploient la même référence. La durée d'appui long est 650 ms et le court
n'est pas déclenché après un long. Les réglages de jeu (permutation, grille,
rotation) continuent à s'appliquer pendant la partie.

Couleurs : bleu = jeu ; vert = stylet ; violet = raccourci court ; orange ambré =
appui long ; gris = aucune affectation. Les noms sur le skin sont abrégés ; les
inspecteurs donnent les libellés complets.
