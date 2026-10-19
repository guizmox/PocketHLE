# VFS / RAM: couverture et validation — 19 octobre 2026

Ces diagnostics sont des tests de non-régression, pas une certification de
Windows CE ou de la Gizmondo. Un PASS ne garantit ni l'absence de corruption
arbitraire dans un jeu, ni l'absence de fuite GPU/audio côté hôte.

## Ajouts automatiques

| Domaine | Scénario et critères vérifiés |
|---|---|
| VFSTEST v4 | 130 contrôles attendus : les 108 précédents + 22. Fichier binaire de 12 291 octets, tous les octets comparés après lecture puis réouverture ; taille des éléments stdio, EOF, flush, modification traversant 4 Kio, origine fseek invalide sans mouvement, erreurs après fermeture, refus d'écriture sur flux en lecture seule, nom Unicode, opérations de longueur zéro, trou de fichier rempli de zéros. |
| RAMTEST v9 | 145 contrôles attendus : les 136 précédents + 9. Tailles 1/7/8/4095/4096/4097/8191/8193, vérification complète des zéros et des données, libérations intercalées, allocations progressives de 1 Mio jusqu'à échec, données antérieures préservées, restitution RAM/espace virtuel, remise à zéro après décommit/recommit. |
| Modèle VFS Rust | 1 024 opérations déterministes sur chacun des stockages Flash et RAM. Lecture, écriture, déplacement, troncature comparés à un modèle indépendant en mémoire ; taille et curseur contrôlés à chaque étape, contenu après fermeture/réouverture. Graine fixe pour reproduire un échec. |
| Fragmentation Rust | 4 096 étapes d'allocations/libérations de tailles mixtes, absence de chevauchement des blocs vivants, restitution exacte et grande allocation finale prouvant la fusion des espaces libres. |
| Échec de commit Rust | Budget physique limité : commit partiel, refus d'une extension excessive sans changement des pages engagées ni du compteur, libération puis nouvelle allocation. |
| Concurrence Rust | Quatre threads hôtes, chacun avec un VFS et un alias du même fichier. 512 enregistrements de 64 octets ; aucune perte, duplication ou écriture déchirée ; fermeture du handle initial pendant que les alias vivent. Ne prétend pas explorer tous les ordonnancements. |
| Arrêt forcé Rust | Sous-processus écrivain arrêté de force après flush, handle encore ouvert. Le parent vérifie les 16 387 octets écrits ; la destruction normale du VFS du fils n'intervient pas. |
| Erreurs hôtes Linux | `/dev/full` provoque une véritable erreur d'écriture. Une limite de taille de fichier appliquée à un sous-processus provoque une véritable écriture partielle puis un échec ; position, taille, contenu et quota ne doivent compter que les octets effectivement écrits. |
| Échec de flush simulé | Empoisonnement volontaire du verrou du backend : flush doit échouer et ne pas altérer les données déjà écrites. C'est une injection d'erreur du backend, pas la reproduction d'un flush disque différé. |
| Confinement Unix | Un lien symbolique vers un répertoire externe ne doit permettre ni lecture, ni création, ni suppression hors montage. |
| API stdio Rust | Appels réels des handlers fwrite/fread/fseek/fflush/fclose avec mémoire invitée ; intégrité complète, lecture après réouverture, EOF et erreurs sur flux fermé. |

Le test d'origine `fseek` invalide a révélé un défaut produit : toute origine
inconnue était interprétée comme SEEK_SET. Le handler refuse désormais cette
valeur, sans modifier le curseur. Aucune tolérance ajoutée aux assertions RAM.

## Ce qui a effectivement été exécuté ici

Linux : 229 tests kernel et 410 tests winceapi réussis, zéro échec ; quatre
tests winceapi restent ignorés par leur configuration existante. Les journaux
sont dans `verification/storage-memory-20261019/`. Le kernel a été relancé
après les deux derniers ajouts ; les modifications ultérieures de ce fichier
ne concernent que la documentation. Les tests stdio ont été exécutés après
correction de fseek ; l'échec attendu avant correction est également conservé.

Le test de fermeture forcée, celui d'écriture partielle et `/dev/full` ont
réellement tourné. Le contrôle rustfmt des fichiers modifiés, la syntaxe C des
diagnostics et la syntaxe Python des scripts ont été vérifiés.

Les versions ARM v4/v9 n'ont PAS été exécutées dans cette session : Clang/LLD
ARM et les fixtures reconstruites ne sont pas disponibles. Le contrôle C avec
le compilateur hôte ne remplace pas une compilation ARM ni l'exécution Unicorn.
Le nombre 130/145 est le résultat attendu à confirmer lors de cette exécution.
Pas de build release complet, pas de lancement GUI Windows/Android ici.

## Relancer les vérifications

Depuis la racine du projet, sous PowerShell ou un terminal Linux :

```powershell
python tools/run-storage-memory-tests.py
```

Pour une passe complète puis neuf répétitions des nouveaux tests ciblés :

```powershell
python tools/run-storage-memory-tests.py --repeat 10
```

Pour reconstruire les diagnostics et exécuter les tests ARM d'intégration,
installer Clang et LLD dans le PATH ainsi que les prérequis du backend Unicorn :

```powershell
python tools/run-storage-memory-tests.py --guest
```

Ce mode reconstruit RAMTEST, VFSTEST et TIMETEST, car le fichier d'intégration
Rust inclut leurs fixtures à la compilation. Il exécute aussi les variantes
de sortie de processus RAM et tous les rapports secondaires. Un prérequis
manquant ou un échec arrête le script ; aucun mode n'est silencieusement ignoré.
Les journaux locaux sont écrits dans `verification/storage-memory-local/`.
Le runner et son aide ont été contrôlés ; sa séquence complète doit être
lancée dans un environnement possédant les prérequis ci-dessus.

## Vérifications restant nécessaires sous Windows et Android

1. Reconstruire/importer les nouveaux ZIP de diagnostic, les lancer deux fois
   en fermant complètement PocketHLE entre les lancements. Vérifier les lignes
   finales exactes, les rapports DLLTEST/DEPTEST/PROCTEST/ORPHANTEST et le compteur
   de persistance. Un ancien ZIP de diagnostic conserve les anciens tests.
2. Refaire les tests sur les emplacements réellement utilisés : stockage interne,
   carte SD si disponible, chemins avec espaces et accents. Conserver l'OS,
   la version du build, le système de fichiers et les journaux pour comparaison.
3. Pour un disque plein/une erreur au flush, utiliser exclusivement un volume
   ou une image de test de petite capacité et des fichiers jetables. Comparer
   l'ancien contenu, la taille, le curseur, les retours API et la réouverture
   après l'erreur. Les tests Linux ne prouvent pas les mêmes erreurs sous NTFS
   ou dans l'accès au stockage Android.
4. Répéter lancement/arrêt des diagnostics et un parcours de jeu pendant une
   session prolongée. Relever la mémoire du processus hôte entre cycles après
   stabilisation (Gestionnaire des tâches/Process Explorer sous Windows,
   `adb shell dumpsys meminfo <package>` sous Android). Chercher une croissance
   répétée, en distinguant caches, mémoire invitée, mémoire native et GPU.
   La seule égalité des compteurs RAM invités ne suffit pas.

## Limites explicites

- Le kill du processus après flush ne simule ni une coupure électrique ni la
  persistance matérielle. Pas de garantie de sauvegarde atomique : il faudrait
  tester le protocole temporaire/renommage propre à chaque application.
- Le test de flush empoisonné ne couvre pas une panne physique ou une erreur
  différée de chaque couche de tampon. Il faudra une injection au niveau d'un
  backend d'E/S abstrait si une telle couche est introduite.
- Aucun fuzzing illimité, aucune exploration exhaustive des courses concurrentes.
- Pas de test général de toutes les combinaisons VirtualProtect/VirtualQuery,
  de tous les formats PE, de toutes les API WinCE ou de tous les chemins Unicode.
- Les tests ne valident pas l'usure réelle d'une mémoire Flash.
