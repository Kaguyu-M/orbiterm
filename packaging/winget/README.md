# Manifestes winget

`Kaguyu-M.Orbiterm` dans [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs).
Après une Release, remplir les modèles (`@VERSION@`, `@HASH@` = SHA-256 de l'installateur, `@DATE@`),
vérifier avec `winget validate --manifest <dossier>` puis soumettre :
`wingetcreate submit --token <jeton GitHub> <dossier>` (ou, une fois le paquet accepté,
`wingetcreate update Kaguyu-M.Orbiterm --version X.Y.Z --urls <url> --submit`).
