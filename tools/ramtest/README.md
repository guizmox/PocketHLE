# Diagnostic RAM invité PocketHLE

`dist/PocketHLE-RAMTEST.zip` est un titre de diagnostic importable dans la GUI.
Le programme exécute de vraies instructions ARM et appelle les API WinCE de
PocketHLE. Il ne nécessite ni SDK propriétaire ni assets commerciaux.

1. Installer les sources du patch puis reconstruire la GUI.
2. Importer `PocketHLE-RAMTEST.zip` comme un jeu ZIP Gizmondo.
3. Lancer le titre. La boîte finale doit afficher **SUCCES**. Fermer avec Entrée.
4. Consulter `RAMTEST.TXT` : la dernière ligne doit être
   `RAMTEST_RESULT PASS checks=0x00000084 failures=0x00000000` (132 contrôles).
5. Après fermeture de la boîte, vérifier `flash/DLLTEST.TXT` : sa dernière
   ligne doit être `DLLTEST_RESULT PASS`. Les callbacks de sortie du processus
   écrivent cette ligne après acquittement de la boîte finale.
6. Vérifier `flash/DEPTEST.TXT` : sa dernière ligne doit être
   `DEPTEST_RESULT PASS` (ordre des dépendances et rollback).
7. Vérifier `flash/PROCTEST.TXT` et `flash/ORPHANTEST.TXT` :
   `PROCTEST_RESULT PASS` et `ORPHANTEST_RESULT PASS`.
8. Relancer une deuxième fois. En cas d'échec, transmettre le rapport et
   `pockethle-gui.log`.

Le rapport est créé dans `\Flash Disk\RAMTEST.TXT`, sur le stockage inscriptible.
Dans la GUI, sa copie hôte se trouve dans `flash/ramtest.txt` sous la racine PocketHLE.
La carte SD contenant l’EXE reste en lecture seule.
Pour retrouver son chemin sous Windows, depuis cmd.exe :

```bat
powershell -NoProfile -Command "Get-ChildItem 'C:\Users\gtristant\Documents\PocketHLE' -Recurse -Filter RAMTEST.TXT | Select-Object -ExpandProperty FullName"
```

La version 7 ajoute une suite multiprocessus : création réelle normale ou
suspendue, commandes/identifiants, duplication explicite, TLS privé, contrôle
distant des threads, attentes/codes de sortie, enfant survivant au parent et
rollback sur manque de RAM ou sortie PROCESS_INFORMATION invalide.
Quatre contrôles RAMTEST encadrent cette suite et sa restitution mémoire.
Les processus ont leurs propres CPU et threads hôtes ; SDCreateProcess conserve
le retour au launcher existant. L’héritage automatique des handles est refusé
conformément au contrat CE ; utiliser DuplicateHandle.

La version 6 ajoute 33 contrôles TLS/erreurs : 64 slots, épuisement,
paramètres invalides, remise à zéro à la réallocation, isolation du main et
de deux workers, écritures directes via KData, réutilisation pendant qu’un
worker attend, conservation du TLS pendant DllMain et erreurs des attentes.
TlsGetValue efface GetLastError en cas de succès, même pour une valeur nulle.
Les autres succès TLS conservent l’erreur. Get/Set suivent la validation minimale
WinCE des indices 0..63, y compris pour un slot non alloué.

Le diagnostic vérifie les notifications DllMain de thread, leur ordre et leur
contexte, puis les detach de processus dans un second rapport DLLTEST.TXT.
Il vérifie les imports natifs par nom/ordinal, les dépendances partagées,
leur conservation par LoadLibrary explicite, les cycles, les dépendances
absentes, les exports absents et le rollback d’un attach rejeté.
La DLL `pageprobe.dll` vérifie le chargement à la première utilisation du
code, des données initialisées et des pages zéro, le décompte unique des pages,
les données modifiées, puis la libération/recharge sans fuite. Le programme
préchauffe ses propres pages avant les mesures pour isoler ces opérations.
Il vérifie aussi réservation/commit/decommit/release, restitution des
allocations et échecs de realloc, isolation de GetLastError entre threads,
32 cycles de piles, 24 cycles de DLL avec références multiples, exports,
rejet de DllMain, erreurs de chargement, redistribution RAM et refus de réduire
la partition programme sous les pages occupées. La partition initiale est
restaurée. Les mesures sont hexadécimales, en octets sauf les nombres de pages
retournés par GetSystemMemoryDivision.

Le marqueur `GZRT999999` sélectionne le profil Gizmondo par la détection normale
des titres. Aucun traitement spécial du diagnostic n'existe dans le noyau.
Le programme est conçu pour les API exposées par PocketHLE ; sa compatibilité
avec un appareil physique n'a pas été vérifiée.

## Reconstruire les binaires

Python 3, Clang et LLD avec cible ARM sont requis. Validation locale avec
Clang/LLD 18.1.3 :

```sh
python3 tools/ramtest/build.py --clang clang --lld ld.lld
```

Le script compile le C freestanding et assemble les en-têtes PE32/WinCE,
imports et exports. Les binaires de `dist/GZRT999999` servent aussi de fixtures
au test d'intégration ; reconstruire après chaque modification du C.

```sh
cargo test -p pocket-core --no-default-features --features unicorn --test ram_guest -- --nocapture
```

Le test monte la carte SD en lecture seule et Flash Disk en écriture, exécute l'EXE avec Unicorn, acquitte la
boîte finale et vérifie rapport, code de sortie, partition restaurée, modules
déchargés et piles de workers libérées. Il supprime ensuite ses fichiers.
Aucune journalisation de diagnostic n'est activée dans les jeux de production.

Les fixtures `dist/fixtures` exécutent aussi ExitProcess(77) et le cas où le
main appelle ExitThread(11) puis le dernier worker retourne 33. Elles ne font
pas partie du ZIP importable. Les trois variantes sont testées avec Unicorn.
