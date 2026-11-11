# Orbiterm face à MobaXterm — fonctionnalités

Base : rapport d'analyse de MobaXterm (`Documents\MobaXterm-Analyse`) et code d'Orbiterm (v0.7.6).
✅ présent · 🟡 partiel · ❌ absent · ⛔ écarté volontairement

## Sessions

| MobaXterm | Orbiterm |
|---|---|
| SSH, Telnet, Rlogin, RDP, VNC, FTP, SFTP, Série, Shell local, Navigateur, Mosh, WSL | ✅ |
| XDMCP (bureau X distant via le serveur X) | ❌ → lot 4 |
| S3 (Amazon) | ⛔ service propriétaire, hors du cœur de l'outil |
| Dossiers, recherche, connexion rapide, sessions récentes | ✅ |
| Paramètres « signet » : macro de démarrage, commentaire | ❌ → lot 1 |
| Import MobaXterm / PuTTY / OpenSSH | ✅ (mRemoteNG, RDM : ⛔ peu utilisés) |
| Export / import des sessions (.moba) | ❌ → lot 3 |
| Sessions partagées (URL, partage réseau) | ⛔ fonction d'entreprise (édition Pro) |

## Terminal

| MobaXterm | Orbiterm |
|---|---|
| Onglets, écran partagé 2 / 4, MultiExec, macros enregistrées | ✅ |
| Recherche dans le terminal | ❌ → lot 1 |
| Journalisation de l'activité dans un dossier | ❌ → lot 1 |
| Coloration syntaxique (mots-clés : error, warning…) | ❌ → lot 1 |
| ZMODEM (lrz / lsz) | ❌ → lot 4 |
| Détacher un onglet en fenêtre | ⛔ tout reste dans les onglets (choix validé) |

## Navigateur SFTP

| MobaXterm | Orbiterm |
|---|---|
| Panneau latéral auto, envoi / téléchargement, glisser-déposer, fichiers cachés | ✅ |
| Ouvrir / éditer un fichier distant (renvoyé à l'enregistrement) | ❌ → lot 2 |
| Suivre le dossier courant du terminal | ❌ → lot 2 |
| Repli SCP quand SFTP est absent | ❌ → lot 4 |

## Bureaux distants

| MobaXterm | Orbiterm |
|---|---|
| RDP intégré, presse-papiers, lecteurs redirigés, rebond SSH | ✅ |
| Passerelle RDP (RD Gateway), multi-écran | ❌ → plus tard |
| Son RDP | 🟡 canal présent, muet → plus tard |
| VNC intégré | ✅ |

## Serveurs intégrés

| MobaXterm | Orbiterm |
|---|---|
| HTTP, TFTP, FTP, SSH/SFTP | ✅ |
| Telnet | ❌ → lot 3 |
| Iperf (test de débit) | ❌ → lot 3 |
| VNC (partage de l'écran local) | ❌ → plus tard (capture d'écran par OS) |
| NFS, Cron | ⛔ démons Cygwin, rôle couvert par le système |

## Outils

| MobaXterm | Orbiterm |
|---|---|
| Scanner de ports, ports ouverts, Wake-on-LAN, tunnels SSH, barre d'infos distante | ✅ |
| Éditeur de texte (MobaTextEditor) | ❌ → lot 2 |
| Comparaison de fichiers / dossiers (MobaDiff) | ❌ → lot 3 |
| Capture de paquets | ⛔ exige un pilote (Npcap) |
| MobApt, jeux, plugins .mxt3, Customizer | ⛔ propres à l'environnement Cygwin de MobaXterm |

## Sécurité / configuration

| MobaXterm | Orbiterm |
|---|---|
| Mots de passe chiffrés (master password) | ✅ autrement : trousseau du système |
| Export de configuration chiffré | ❌ → lot 3 (sans mots de passe) |
| Vérification des mises à jour | ⛔ pas de serveur de publication |

## Lots de travail

1. **Terminal** : recherche, journalisation des sessions, surlignage de mots-clés, macro de démarrage et commentaire par session.
2. **Fichiers** : éditeur de texte intégré (onglet), édition des fichiers distants depuis le SFTP, suivi du dossier du terminal.
3. **Échanges / outils** : export-import des sessions et de la configuration, comparaison de fichiers et dossiers, serveurs Telnet et Iperf.
4. **Protocoles** : XDMCP, ZMODEM, repli SCP.
