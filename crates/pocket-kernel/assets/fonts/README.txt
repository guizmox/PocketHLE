Polices de remplacement embarquées dans PocketHLE

Arial / Helvetica -> Liberation Sans 2.1.5
Times New Roman / MS Serif -> Liberation Serif 2.1.5
Courier New / Courier -> Liberation Mono 2.1.5
Tahoma / MS Sans Serif -> Wine Tahoma

Les variantes Liberation normale, grasse, italique et grasse italique
sont fournies sous licence SIL OFL 1.1. Provenance des binaires :
https://deb.debian.org/debian/pool/main/f/fonts-liberation/fonts-liberation_2.1.5-3_all.deb
Projet et sources : https://github.com/liberationfonts/liberation-fonts
Voir LICENSE-Liberation.txt. Fichiers originaux non modifiés.

Wine Tahoma normale et grasse : https://github.com/wine-mirror/wine/tree/master/fonts
Fichiers tahoma.ttf, tahomabd.ttf, tahoma.sfd, tahomabd.sfd.
Copyright (c) 2004 Larry Snyder; basé sur Bitstream Vera Sans,
Copyright (c) 2003 Bitstream, Inc. Voir les deux licences jointes.
Les sources SFD sont incluses. Pour reconstruire avec FontForge :
fontforge -lang=ff -c 'Open($1); Generate($2)' WineTahoma-Regular.sfd tahoma.ttf
fontforge -lang=ff -c 'Open($1); Generate($2)' WineTahoma-Bold.sfd tahomabd.ttf
Le rendu italique de Wine Tahoma est synthétisé par cisaillement.

Choix facultatif de polices externes : variable POCKETHLE_FONTS_DIR.
Noms reconnus : arial.ttf / arialbd.ttf / ariali.ttf / arialbi.ttf,
tahoma.ttf / tahomabd.ttf, times.ttf / timesbd.ttf / timesi.ttf / timesbi.ttf,
cour.ttf / courbd.ttf / couri.ttf / courbi.ttf.
Sans cette variable, les substitutions embarquées sont identiques sous
Windows et Android. Leurs métriques ne garantissent pas un rendu identique
au pixel près à WinCE. Le moteur utilise ab_glyph sans hinting TrueType.
