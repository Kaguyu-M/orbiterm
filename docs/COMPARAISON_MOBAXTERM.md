# Orbiterm face à MobaXterm — fonctionnalités

Base : rapport d'analyse de MobaXterm (`Documents\MobaXterm-Analyse`) et code d'Orbiterm (v0.7.6, mis à jour jusqu’à la v1.0.1 et la version en cours).
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
| Raccourcis clavier personnalisables | ✅ 0.9.6 (Aide → Raccourcis clavier : plusieurs raccourcis par action, conflits résolus, repris par les menus et les terminaux) |
| Fin de session : onglet fermé, retour à l'accueil (`exit`, déconnexion) | ✅ 0.8.7 (une coupure ou une erreur garde l'onglet avec sa raison et « Redémarrer ») |
| Détacher un onglet en fenêtre | ⛔ tout reste dans les onglets (choix validé) |

## SSH

| MobaXterm | Orbiterm |
|---|---|
| Clés privées OpenSSH et PuTTY (.ppk) | ✅ 0.7.14 (.ppk v2 / v3, chiffrées ou non) |
| Générateur de clés (MobaKeyGen) | ✅ 0.7.14 (Outils → Générateur de clés SSH : Ed25519, ECDSA, RSA ; OpenSSH ou .ppk ; conversion et changement de phrase secrète) |
| Agent SSH (MobAgent, Pageant), transfert d'agent | ✅ 0.7.14 (agent Orbiterm : clés chargées en mémoire, au démarrage si voulu ; agent du système : OpenSSH / Pageant / SSH_AUTH_SOCK ; transfert d'agent en option) |
| Encadré d'accueil de la session SSH (options actives : X11, compression, SFTP…) | ✅ 0.9.7 (rebond, compression, SFTP, X11 accepté ou refusé par le serveur, transfert d'agent ; désactivable) |
| Coloration du terminal | ✅ 0.9.7 étendue : états (running, stopped…), adresses IPv4 / IPv6 / MAC, dates, liens, invite du shell quand le serveur ne la colore pas |
| Vérification en deux étapes (code à usage unique, PIN, Duo : questions « keyboard-interactive » du serveur) | ✅ version en cours (fenêtre de saisie ; le mot de passe mémorisé répond seul à « Password » ; seconde méthode après une clé ou l'agent) |
| Délais de connexion (serveur injoignable) | ✅ 0.8.7 (15 s pour joindre le serveur, 30 s de silence pendant l'échange de clés, 90 s pour l'authentification) |

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
| Progression des transferts | ✅ 0.9.7 (pourcentage, vitesse, temps restant) |

## Bureaux distants

| MobaXterm | Orbiterm |
|---|---|
| RDP intégré, presse-papiers, lecteurs redirigés, rebond SSH | ✅ (presse-papiers avec Windows en transport UDP : 1.0.1) |
| Passerelle RDP (RD Gateway), multi-écran | ✅ 0.7.13 : multi-écran (tous les écrans du poste, plein écran sur tous) ; passerelle RD (HTTPS/WebSocket, Basic/NTLM/Negotiate, compte de la session ou compte propre, certificat vérifié ; aussi pour mstsc / FreeRDP) |
| Son RDP | ✅ 0.7.11 (lu dans l'onglet ; option de session « Son ») ; avec le bureau à distance d'Ubuntu (GNOME Remote Desktop, canal AUDIO_PLAYBACK_DVC) : version en cours |
| Image transmise en H.264 (pipeline graphique, comme mstsc) | ✅ 0.8.9 Windows (décodage par Windows), 0.9.1 Linux (GStreamer : VA-API ou processeur) ; 10 à 30 fois moins de données que les images bitmap |
| Affichage par la carte graphique | ✅ 0.8.8 (WebGL), 0.9.3 image composée par la carte graphique (Windows et Linux) |
| Qualité de l'image par session (réseau local, lien lent / VPN, automatique) | ✅ 0.8.8 (l'automatique mesure le lien ; le serveur adapte l'image pendant la session, 0.9.0) |
| Plein écran comme le Bureau à distance de Windows | ✅ 0.9.0 : barre de connexion escamotable en haut, touche Windows / Super, Alt+Tab, Ctrl+Échap, Alt+F4 envoyés à l'ordinateur distant ; F11 (0.9.6) |
| Raccourcis Windows envoyés à l'ordinateur distant même en fenêtre (comme RustDesk) | ✅ version en cours : par défaut sous Windows dès que le bureau distant a le clavier ; Ctrl+Alt+Pause bascule le plein écran |
| Échelle d'affichage transmise au serveur (125 %, 150 %) | ✅ 0.9.0 (suit le passage de la fenêtre sur un autre écran ; option automatique / 100 %) |
| Reconnexion automatique (veille, coupure Wi-Fi) | ✅ 0.9.0 (2 s, 5 s, 15 s… bandeau « Connexion perdue » avec Annuler ; activée par défaut en RDP, optionnelle en SSH) |
| Transport UDP (comme mstsc) | ✅ 0.9.5 (repli TCP automatique ; utile sur VPN et liens avec pertes) |
| Cache d'images persistant (reconnexion plus rapide) | ✅ 0.9.3 |
| Bureau à distance d'Ubuntu (GNOME Remote Desktop, connexion à distance) | ✅ 0.9.8 – 1.0.0 (écran de connexion puis bureau ; redirection et RDSTLS ; « Fermer la session » sans reconnexion) |
| Fermer la session distante depuis la barre (Windows ou Linux) | ✅ |
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
| Vérification des mises à jour | ✅ 0.7.9 (Aide → Rechercher les mises à jour) ; 0.8.4 vérification au démarrage (désactivable) ; 0.8.6 mise à jour Windows sans assistant, installateur vérifié par SHA-256 |

## Plateformes et installation

| MobaXterm | Orbiterm |
|---|---|
| Windows uniquement (installateur, version portable) | ✅ Windows (installateur, version portable, Scoop ; winget bientôt) et **Linux** : dépôts signés APT (Debian, Ubuntu), DNF (Fedora), pacman (Arch, Omarchy ; AUR bientôt), Nix / NixOS (flake ou `configuration.nix`) |
| Tests avant publication | ✅ 0.8.7 : chaque publication passe d'abord les tests sous Windows et Linux |

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
14. ✅ (0.8.4) **Mise à jour depuis l'application** : vérification au démarrage (désactivable dans Paramètres › Général, « Ignorer cette version » possible) ; l'installation Windows télécharge le nouvel installateur, compare son SHA-256 à celui de la publication, le lance et se ferme. Les autres installations (Scoop, portable, .deb, .rpm, Nix) affichent la commande à lancer. La vérification repose sur GitHub : pas encore de signature propre à l'application. Depuis 0.8.6, la mise à jour Windows se fait sans assistant (barre de progression seule, puis redémarrage).
15. ✅ (0.8.7) **Fin de session et délais** : quitter une session ferme l'onglet et revient à l'accueil ; délais de connexion SSH et RDP ; tests exigés avant chaque publication ; NixOS sans flake.
16. ✅ (0.8.8 – 0.9.3) **Bureau à distance fluide** : affichage par la carte graphique, réglage de qualité, H.264 (Windows puis Linux par GStreamer), cache d'images persistant, rythme calé sur l'affichage.
17. ✅ (0.9.0 – 0.9.2) **Bureau à distance comme mstsc** : plein écran avec barre de connexion et raccourcis envoyés à l'ordinateur distant, échelle d'affichage, reconnexion automatique, mesure du lien ; paquet Arch / Omarchy (dépôt pacman signé).
18. ✅ (0.9.4 – 0.9.5) **Transport UDP** du bureau à distance ; plus de reconnexion intempestive quand la fenêtre est masquée.
19. ✅ (0.9.6 – 0.9.7) **Raccourcis clavier modifiables**, progression des transferts, encadré d'accueil SSH et coloration étendue.
20. ✅ (0.9.8 – 1.0.0) **Bureau à distance d'Ubuntu** (GNOME Remote Desktop) : redirection vers l'écran de connexion puis la session, authentification RDSTLS, fermeture de session propre.

21. ✅ (1.0.1) **Presse-papiers RDP avec Windows en UDP** : Windows passe alors par un canal dynamique « cliprdr » mais écoute les réponses sur le canal statique.
22. ✅ (version en cours) **Raccourcis Windows en fenêtre** (comme RustDesk), **vérification SSH en deux étapes**, **son du bureau à distance d'Ubuntu**.

Reste : paquets winget et AUR (en attente de Microsoft et de la réouverture des inscriptions AUR).
