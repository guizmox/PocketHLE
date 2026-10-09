PocketHLE — lot VFS et VFSTEST v1

Patch CUMULATIF depuis PocketHLE(1).zip. Il conserve les lots précédents :
RAM, images/piles, DLL/dépendances, pagination, TLS/erreurs et multiprocessus.
Les manifestes et harness temporaires, compilateurs et caches sont exclus.

Corrections des défauts observés dans l'audit :
- CreateFileW respecte les cinq dispositions sans troncature accidentelle.
- Partage lecture/écriture contrôlé entre processus et handles dupliqués.
- fopen : r+ ne crée pas, w+ tronque, a/a+ ajoutent même après un seek.
- Erreurs file/path/find cohérentes ; pointeurs protégés vérifiés avant mutation.
- RemoveDirectoryW supprime réellement les répertoires vides et refuse les autres.
- SetFilePointer refuse une position négative sans modifier la position courante.
- MoveFileW refuse une destination existante et les fichiers encore ouverts.
- GetDiskFreeSpaceExW rapporte les volumes modélisés et l'espace réellement occupé.

Flash Disk : 32 MiB utilisables sur la NAND de 64 MiB, les 32 MiB OS restant
réservés. Quota logique indépendant de la RAM, contrôlé sur écriture/troncature,
avec restitution après réduction/suppression. SD : capacité synthétique arrondie
à la puissance de deux supérieure, minimum 64 MiB, selon le contenu monté.
Ce n'est pas une détection de la capacité d'une carte physique. Les attributs
supplémentaires sont partagés dans la session sans persistance complète au reboot.
Le lot corrige les constats de l'audit ; il ne prétend pas implémenter toutes
les API WinCE de fichiers. Aucun traceur temporaire de production ajouté.

Validation locale : 375 tests distincts passent (kernel 136, winceapi 215,
core 3, CPU 12, runner desktop 5, intégration ARM 4). CPU, core, runner et
intégration ARM passent dans les deux modes TLB Unicorn. VFSTEST vérifie
103 contrôles et est exécuté deux fois dans chaque mode ; RAMTEST v7 conserve
ses 132 contrôles dans les trois variantes de sortie. La GUI Windows est à
reconstruire et vérifier sur ta machine.

Installation depuis CMD à la racine du dépôt, après sauvegarde des sources :

powershell -NoProfile -Command "Expand-Archive -LiteralPath 'PocketHLE-vfs.zip' -DestinationPath '.' -Force"
powershell -NoProfile -Command "Get-Content 'patch-files.txt' | ForEach-Object { (Get-Item -LiteralPath $_).LastWriteTime = Get-Date }"
cargo build --release -p pocket-desktop
set POCKETHLE_GIZMONDO_RAM_TRACE=
set RUST_LOG=info
target\release\pockethle-gui.exe

Importer PocketHLE-VFSTEST-v1.zip dans la bibliothèque de jeux, puis lancer
DEUX fois. À chaque fois, attendre SUCCES puis fermer avec Entrée.
Rapport : C:\Users\gtristant\Documents\PocketHLE\flash\VFSTEST.TXT
Résultat attendu :
VFSTEST_RESULT PASS checks=0x00000067 failures=0x00000000

Ce test crée uniquement son répertoire PocketHLE-VFS-PROBE et son rapport.
Il refuse un répertoire de test préexistant pour ne pas supprimer des fichiers
inconnus. Après succès, seuls son rapport et les fichiers déjà présents restent.
En cas d'échec, transmettre VFSTEST.TXT et pockethle-gui.log ; ne pas effacer un
répertoire de test sans en vérifier le contenu. Les sauvegardes ne sont pas utilisées.

Documentation et reconstruction du binaire ARM : tools\vfstest\README.md
Test source optionnel :
cargo test -p pocket-core --no-default-features --features unicorn --test ram_guest native_arm_vfs -- --nocapture

Fichiers remplacés/créés :
crates\pocket-cpu\src\lib.rs
crates\pocket-cpu\src\stub.rs
crates\pocket-cpu\src\unicorn.rs
crates\pocket-kernel\src\audio.rs
crates\pocket-kernel\src\handles.rs
crates\pocket-kernel\src\lib.rs
crates\pocket-kernel\src\shared_objects.rs
crates\pocket-kernel\src\vfs.rs
crates\pocket-winceapi\src\coredll.rs
crates\pocket-winceapi\src\gx.rs
crates\pocket-winceapi\src\lib.rs
crates\pocket-winceapi\src\wavein.rs
docs\AGENTS.md
frontends\pocket-desktop\src\runner.rs
crates\pocket-kernel\src\dll_lifecycle.rs
crates\pocket-winceapi\src\ddraw.rs
crates\pocket-core\tests\ram_guest.rs
crates\pocket-cpu\src\image_pages.rs
crates\pocket-kernel\src\image_memory.rs
crates\pocket-kernel\src\memory_division.rs
crates\pocket-kernel\src\tls.rs
crates\pocket-core\src\lib.rs
crates\pocket-kernel\src\process_control.rs
tools\ramtest\README.md
tools\ramtest\build.py
tools\ramtest\build_dependencies.py
tools\ramtest\build_paging.py
tools\ramtest\dist\GZRT999999\AUTORUN.EXE
tools\ramtest\dist\GZRT999999\GZRT999999
tools\ramtest\dist\GZRT999999\badproc.exe
tools\ramtest\dist\GZRT999999\depbadexport.dll
tools\ramtest\dist\GZRT999999\depcyclea.dll
tools\ramtest\dist\GZRT999999\depcycleb.dll
tools\ramtest\dist\GZRT999999\depleaf.dll
tools\ramtest\dist\GZRT999999\depmissing.dll
tools\ramtest\dist\GZRT999999\deppeer.dll
tools\ramtest\dist\GZRT999999\depreject.dll
tools\ramtest\dist\GZRT999999\deproot.dll
tools\ramtest\dist\GZRT999999\pageprobe.dll
tools\ramtest\dist\GZRT999999\proctest.exe
tools\ramtest\dist\GZRT999999\procworker.exe
tools\ramtest\dist\GZRT999999\ramprobe.dll
tools\ramtest\dist\GZRT999999\ramprobe2.dll
tools\ramtest\dist\GZRT999999\ramreject.dll
tools\ramtest\dist\PocketHLE-RAMTEST.zip
tools\ramtest\dist\fixtures\explicit-exit.exe
tools\ramtest\dist\fixtures\last-worker-exit.exe
tools\ramtest\src\dep.c
tools\ramtest\src\imports.json
tools\ramtest\src\proctest.c
tools\ramtest\src\procworker.c
tools\ramtest\src\ramprobe.c
tools\ramtest\src\ramtest.c
crates\pocket-kernel\src\vfs_contract.rs
tools\vfstest\README.md
tools\vfstest\build.py
tools\vfstest\dist\GZVT999998\AUTORUN.EXE
tools\vfstest\dist\GZVT999998\GZVT999998
tools\vfstest\dist\GZVT999998\asset.bin
tools\vfstest\dist\GZVT999998\vfsworker.exe
tools\vfstest\dist\PocketHLE-VFSTEST.zip
tools\vfstest\src\imports.json
tools\vfstest\src\vfstest.c
