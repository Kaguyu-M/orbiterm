# Orbiterm face à MobaXterm — fonctionnalités

Base : rapport d'analyse de MobaXterm (`Documents\MobaXterm-Analyse`) et code d'Orbiterm (v0.7.6, mis à jour jusqu’à la v0.7.14).
✅ présent · 🟡 partiel · ❌ absent · ⛔ écarté volontairement

## Sessions

| MobaXterm | Orbiterm |
|---|---|
| SSH, Telnet, Rlogin, RDP, VNC, FTP, SFTP, Série, Shell local, Navigateur, Mosh, WSL | ✅ |
| S3 (Amazon) | ⛔ service propriétaire, hors du cœur de l'outil |
| Dossiers, recherche, connexion rapide, sessions récentes | ✅ |
| Paramètres « signet » : macro de démarrage, commentaire | ✅ 0.7.7 |
| Import MobaXterm / PuTTY / OpenSSH | ✅ (mRemoteNG, RDM : ⛔ peu utilisés) |
| Export / import des sessions (.moba) | ✅ 0.7.9 (fichier Orbiterm `.json` : sessions, dossiers, macros, tunnels, préférences ; import des `.moba` déjà présent) |
| Sessions partagées (URL, partage réseau) | ⛔ fonction d'entreprise (édition Pro) |

## Terminal

| MobaXterm | Orbiterm |
|---|---|
| Onglets, écran partagé 2 / 4, MultiExec, macros enregistrées | ✅ |
| Recherche dans le terminal | ✅ 0.7.7 |
| Journalisation de l'activité dans un dossier | ✅ 0.7.7 |
| Coloration syntaxique (mots-clés : error, warning…) | ✅ 0.7.7 |
| ZMODEM (lrz / lsz) | ✅ 0.7.10 (`rz` / `sz` dans le terminal, Ctrl+C interrompt, jamais d'écrasement) |
| Détacher un onglet en fenêtre | ⛔ tout reste dans les onglets (choix validé) |

## SSH

| MobaXterm | Orbiterm |
|---|---|
| Clés privées OpenSSH et PuTTY (.ppk) | ✅ 0.7.14 (.ppk v2 / v3, chiffrées ou non) |
| Générateur de clés (MobaKeyGen) | ✅ 0.7.14 (Outils → Générateur de clés SSH : Ed25519, ECDSA, RSA ; OpenSSH ou .ppk ; conversion et changement de phrase secrète) |
| Agent SSH (MobAgent, Pageant), transfert d'agent | ✅ 0.7.14 (agent Orbiterm : clés chargées en mémoire, au démarrage si voulu ; agent du système : OpenSSH / Pageant / SSH_AUTH_SOCK ; transfert d'agent en option) |

## Navigateur SFTP

| MobaXterm | Orbiterm |
|---|---|
| Panneau latéral auto, envoi / téléchargement, glisser-déposer, fichiers cachés | ✅ |
| Ouvrir / éditer un fichier distant (renvoyé à l'enregistrement) | ✅ 0.7.8 (double-clic → onglet éditeur, Ctrl+S renvoie le fichier) |
| Suivre le dossier courant du terminal | ✅ 0.7.8 (serveurs Linux) |
| Repli SCP quand SFTP est absent | ✅ 0.7.10 (automatique, signalé « Mode SCP » dans le navigateur) |
| Menu contextuel (clic droit) sur les fichiers et le fond de la liste | ✅ 0.7.15 |
| Droits d'accès (chmod) : cases lecture / écriture / exécution et valeur octale | ✅ 0.7.15 (SFTP, SCP, FTP `SITE CHMOD`) |
| Nouveau fichier vide, copier le chemin | ✅ 0.7.15 |
| Téléchargement et envoi de dossiers entiers (bouton, menu, glisser-déposer) | ✅ 0.7.15 |
| Ouvrir avec le programme par défaut du poste (renvoyé à chaque enregistrement) | ✅ 0.7.15 |
| Onglet SFTP / FTP à deux volets : explorateur du poste local à côté des fichiers distants, glisser-déposer entre les deux | ✅ 0.8.0 |
| Sélection multiple dans les listes de fichiers (Ctrl / Maj + clic, Ctrl + A) | ✅ 0.8.1 |

## Bureaux distants

| MobaXterm | Orbiterm |
|---|---|
| RDP intégré, presse-papiers, lecteurs redirigés, rebond SSH | ✅ |
| Passerelle RDP (RD Gateway), multi-écran | ✅ 0.7.13 : multi-écran (tous les écrans du poste, plein écran sur tous) ; passerelle RD (HTTPS/WebSocket, Basic/NTLM/Negotiate, compte de la session ou compte propre, certificat vérifié ; aussi pour mstsc / FreeRDP) |
| Son RDP | ✅ 0.7.11 (lu dans l'onglet ; option de session « Son ») |
| VNC intégré | ✅ |

## Serveurs intégrés

| MobaXterm | Orbiterm |
|---|---|
| HTTP, TFTP, FTP, SSH/SFTP | ✅ (SSH : clés publiques autorisées ✅ 0.7.14) |
| Telnet | ✅ 0.7.9 (identifiant + mot de passe, shell du poste) |
| Iperf (test de débit) | ✅ 0.7.9 (compatible clients iperf3, TCP, -P, -R ; pas d'UDP) |
| VNC (partage de l'écran local) | ✅ 0.7.12 (Windows ; Linux sous X11 — pas Wayland) : affichage seul ou prise de contrôle, mot de passe obligatoire |
| NFS, Cron | ⛔ démons Cygwin, rôle couvert par le système |

## Outils

| MobaXterm | Orbiterm |
|---|---|
| Scanner de ports, ports ouverts, Wake-on-LAN, tunnels SSH, barre d'infos distante | ✅ |
| Éditeur de texte (MobaTextEditor) | ✅ 0.7.8 (onglet éditeur intégré) |
| Comparaison de fichiers / dossiers (MobaDiff) | ✅ 0.7.9 (onglet : côte à côte, recopie de blocs, dossiers par contenu) |
| Capture de paquets | ⛔ exige un pilote (Npcap) |
| MobApt, jeux, plugins .mxt3, Customizer | ⛔ propres à l'environnement Cygwin de MobaXterm |

## Sécurité / configuration

| MobaXterm | Orbiterm |
|---|---|
| Mots de passe chiffrés (master password) | ✅ autrement : trousseau du système |
| Gestion des mots de passe enregistrés (liste, suppression) | ✅ 0.7.16 (Paramètres → Gestion des mots de passe : liste, remplacement, suppression ; les valeurs ne sont jamais affichées) |
| Export de configuration chiffré | ✅ 0.7.9 autrement : export sans aucun mot de passe (rien à chiffrer) |
| Vérification des mises à jour | ✅ 0.7.9 (Aide → Rechercher les mises à jour, sur demande uniquement ; publications GitHub) |

## Lots de travail

1. ✅ (0.7.7) **Terminal** : recherche, journalisation des sessions, surlignage de mots-clés, macro de démarrage et commentaire par session.
2. ✅ (0.7.8) **Fichiers** : éditeur de texte intégré (onglet), édition des fichiers distants depuis le SFTP, suivi du dossier du terminal.
3. ✅ (0.7.9) **Échanges / outils** : export-import des sessions et de la configuration, comparaison de fichiers et dossiers, serveurs Telnet et Iperf.
4. ✅ (0.7.10) **Protocoles** : ZMODEM, repli SCP.
5. ✅ (0.7.11 – 0.7.13) **Bureau à distance et partage** : son RDP, serveur VNC (partage de l'écran de ce poste), RDP multi-écran et passerelle RD Gateway.
6. ✅ (0.7.14) **Clés SSH** : générateur de clés, clés PuTTY, clés publiques sur le serveur SSH intégré (0.7.14) ; agent SSH (agent du système / Pageant, transfert d'agent).
7. ✅ (0.7.15) **Navigateur SFTP complet** : menu contextuel, droits d'accès, nouveau fichier, copie du chemin, dossiers entiers, ouverture avec le programme par défaut.
8. ✅ (0.7.16) **Gestion des mots de passe** : fenêtre listant les mots de passe du trousseau (sessions, passerelles, VNC, serveurs intégrés), remplacement et suppression.
9. ✅ (0.7.17) **Serveur X** : installation de VcXsrv en un clic (winget) depuis Paramètres → Serveur X, avec suivi de la progression.
10. ✅ (0.7.18) **Thèmes** : thèmes d'interface Cyberpunk néon, Gris sombre et Gris clair (en plus de crème et brun), thèmes de terminal assortis.
11. ✅ (0.8.0) **Explorateur local intégré** : onglets SFTP / FTP à deux volets (poste local / serveur), transferts par glisser-déposer entre les volets.
12. ✅ (0.8.2) **Rangement des sessions** : glisser les sessions (une ou plusieurs) et les dossiers dans les dossiers, « Déplacer vers… » — le glisser-déposer précédent ne fonctionnait pas sous Windows.
13. ✅ (0.8.3) **Menu du clic droit dans la liste des sessions**, y compris sur la zone vide : nouvelle session, nouveau dossier, **export de sessions vers un fichier** (une session, la sélection, un dossier avec ses sous-dossiers, ou tout) et **import depuis un fichier** (dans le dossier cliqué, sans doublon). Les mots de passe ne sont jamais écrits dans le fichier.
