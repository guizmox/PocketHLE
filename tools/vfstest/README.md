# VFSTEST v1 — diagnostic ARM du lot VFS

Importer `PocketHLE-VFSTEST-v1.zip` dans PocketHLE, puis lancer le test deux fois.
À chaque lancement, attendre **SUCCES**, puis fermer la boîte avec Entrée.
Le même programme teste les fichiers, les volumes et les échanges entre processus.

Le rapport est `\Flash Disk\VFSTEST.TXT`, soit normalement
`C:\Users\gtristant\Documents\PocketHLE\flash\VFSTEST.TXT` :

```text
VFSTEST_RESULT PASS checks=0x00000067 failures=0x00000000
```

Les 103 contrôles couvrent les dispositions CreateFileW, les modes CRT r+/w+/a/a+,
le partage et la duplication locale/distante, les erreurs et les pointeurs de sortie
invalides ou protégés, les recherches et attributs, la suppression réelle des
répertoires, le déplacement sans écrasement et les déplacements de position négatifs.
Ils vérifient aussi les fichiers RAM partagés, la comptabilité des volumes, la limite
Flash de 32 MiB, le refus d'une écriture quand elle est pleine et la restitution
de l'espace après troncature/suppression, sans consommer cette capacité en RAM.

Le test utilise exclusivement son répertoire `\Flash Disk\PocketHLE-VFS-PROBE`
et son rapport. Si ce répertoire existe déjà, il refuse de démarrer pour éviter
de supprimer des fichiers inconnus. Après succès, il retire ses fichiers et
répertoires temporaires et conserve seulement le rapport. Le rapport précédent
est remplacé. `asset.bin` sur la carte sert au contrôle du montage en lecture seule.
Les sauvegardes des jeux ne sont pas utilisées.

La NAND de 64 MiB est modélisée avec 32 MiB disponibles pour Flash Disk et
32 MiB réservés à l'OS. Cette capacité de stockage est distincte du budget RAM.
Le volume SD a une capacité synthétique, arrondie à la puissance de deux supérieure
avec un minimum de 64 MiB selon son contenu : ce n'est pas une détection de la
capacité d'une carte physique. Les attributs supplémentaires sont partagés pendant
la session ; leur persistance complète entre redémarrages n'est pas implémentée.
Ce diagnostic valide les défauts observés dans l'audit, sans prétendre couvrir
toutes les API WinCE de fichiers.

Compilation du diagnostic sans SDK propriétaire :

```text
python tools/vfstest/build.py --clang clang --lld ld.lld
```

Le ZIP produit est `tools/vfstest/dist/PocketHLE-VFSTEST.zip`.
Régression native (exécute le binaire ARM deux fois et vérifie le nettoyage et la RAM) :

```text
cargo test -p pocket-core --no-default-features --features unicorn --test ram_guest native_arm_vfs -- --nocapture
```

Contrats WinCE de référence :

- https://learn.microsoft.com/en-us/previous-versions/ms959950(v=msdn.10)
- https://learn.microsoft.com/en-us/previous-versions/ms961237(v=msdn.10)
- https://learn.microsoft.com/en-us/previous-versions/windows/embedded/ms891933(v=msdn.10)
- https://learn.microsoft.com/en-us/previous-versions/ms890887(v=msdn.10)
