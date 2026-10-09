# CAMTEST — caméra Gizmondo CAM1

Importer `dist/PocketHLE-CAMTEST.zip` dans la librairie, comme un jeu Gizmondo.
Activer **Emulator options → Camera hardware (CAM1)** avant de le lancer.
Windows utilise la première webcam ; Android préfère la caméra arrière et
demande l’autorisation CAMERA. Une caméra occupée, absente ou interdite reste
une erreur réelle. Fermer les autres applications qui utilisent la caméra.

Le test exécute les appels ARM du pilote, vérifie le format, récupère deux
aperçus à une cadence maximale de 20 ips, capture une image I420, puis arrête
et ferme la caméra. Résultats dans le dossier `flash` de la librairie :

- `CAMTEST.TXT` : attendre `CAMTEST_RESULT PASS`.
- `CAMTEST-preview.bmp` : ouvrir ce BMP pour contrôler l’image réelle ;
  RGB565, 320×240, lignes bottom-up et hauteur BMP positive comme dans le SDK.
  Utiliser ce CAMTEST mis à jour avec le correctif d’orientation du pilote.
- `CAMTEST-capture.i420` : Y/U/V 640×480, 460800 octets.

Sous Windows les noms peuvent être affichés en minuscules. Un PASS confirme
les appels et la capture ; contrôler aussi visuellement le BMP. Pour vérifier
la libération du périphérique, relancer le test et ouvrir ensuite l’application
Caméra Windows. Sur Android, vérifier également pause/reprise avec un jeu qui
utilise un aperçu continu ; ce diagnostic termine et ferme sa capture avant
d’afficher son message final.

Pour visualiser la capture brute avec FFmpeg installé :

```text
ffmpeg -f rawvideo -pixel_format yuv420p -video_size 640x480 -i CAMTEST-capture.i420 -frames:v 1 CAMTEST-capture.png
```

La source C est indépendante du SDK propriétaire. Pour reconstruire le binaire
avec Python, Clang et LLD disponibles :

```text
python tools/camtest/build.py --clang clang --lld ld.lld
```

Validation locale : exécution ARM réelle avec Unicorn et caméra synthétique,
15 vérifications réussies, dimensions et couleurs du BMP vérifiées. Le test
matériel Windows/Android et la construction complète de l’APK restent à faire
sur ces systèmes.
