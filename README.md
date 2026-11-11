# Orbiterm

Terminal multi-protocoles multiplateforme (Windows + Linux) : onglets, sessions enregistrées, navigateur SFTP, tunnels SSH,
X11, RDP, VNC, FTP, navigateur web, écran partagé, MultiExec et macros. Application originale dont l'ergonomie s'inspire de MobaXterm.
Conception : [`docs/CONCEPTION.md`](docs/CONCEPTION.md).

**Version 0.7.10**

| Domaine | Fonctionnalités |
|---|---|
| Sessions | SSH, Telnet, Rlogin, port série, Mosh, shells locaux (PowerShell, cmd, Git Bash…), WSL ; dossiers, glisser-déposer, recherche, connexion rapide, sessions récentes |
| SSH | mot de passe (trousseau du système) ou clé (OpenSSH ou PuTTY `.ppk`), **agent SSH** (agent Orbiterm qui garde les clés déverrouillées, agent OpenSSH / Pageant du système, transfert d’agent en option), vérification des clés d'hôte, navigateur SFTP, compression, commande distante, keepalive ; bandeau au-dessus du terminal (copier, coller, effacer, session SFTP, plein écran, fermer la session), barre d’informations du serveur sous le terminal (CPU, mémoire, disque, réseau, durée de fonctionnement, utilisateurs) ; depuis un onglet SSH : **session SFTP séparée** enregistrée (même serveur, même compte, mot de passe mémorisé dans le trousseau) |
| Bureaux distants | RDP **intégré dans l'onglet** (client IronRDP : NLA, certificat vérifié comme les clés SSH, mot de passe dans le trousseau, bureau adapté à la taille de l'onglet, plein écran, **multi-écran** (tous les écrans du poste), **passerelle RD** (RD Gateway), Ctrl+Alt+Suppr, **presse-papiers partagé** (texte, dans les deux sens), **son de l’ordinateur distant** lu dans l’onglet (option « Son »), **partage d’un dossier local** (lecteur redirigé : « Ce PC » sous Windows, ~/thinclient_drives avec xrdp), coller du texte, bouton **Fermer la session** (fermeture de la session Windows ou Linux, ou simple déconnexion), barre d’informations du poste distant ; option fenêtre séparée mstsc / FreeRDP), VNC intégré dans l'onglet (noVNC : mot de passe mémorisable, ajustement, lecture seule, Ctrl+Alt+Suppr, presse-papiers) |
| Fichiers | onglets SFTP et FTP / FTPS autonomes à **deux volets** : l'explorateur du poste local (emplacements et disques, nouveau dossier, renommer, corbeille, masquable et redimensionnable) à côté des fichiers distants, transferts en **glissant d'un volet à l'autre** ou avec les boutons Envoyer / Télécharger (explorateur plein onglet avec colonnes, envoi / téléchargement / glisser-déposer de fichiers **et de dossiers entiers**, menu contextuel, droits d’accès (chmod), nouveau fichier, copie du chemin, ouverture avec le programme par défaut du poste), édition des fichiers distants (double-clic, renvoyés au serveur à l’enregistrement), navigateur SFTP qui suit le dossier du terminal (serveurs Linux), repli automatique sur SCP et les commandes du shell quand le serveur ne propose pas SFTP (dropbear, équipements réseau) |
| Éditeur | onglet éditeur de texte (coloration syntaxique, rechercher / remplacer, aller à la ligne, encodage et fins de ligne conservés), fichiers locaux (Ctrl+Maj+O) ou distants |
| Comparaison | onglet de comparaison de deux fichiers (côte à côte, recopie d'un bloc d'un côté à l'autre, enregistrement) ou de deux dossiers (fichiers identiques, différents, absents d'un côté ; double-clic pour comparer un fichier) |
| Navigateur | onglet web intégré (précédent / suivant / actualiser / barre d'adresse), sans aucun accès aux commandes d'Orbiterm |
| Réseau | redirections -L / -R / -D (SOCKS) par session, gestionnaire de tunnels (démarrage auto), rebond SSH (jump host, aussi pour RDP / VNC), redirection X11 (VcXsrv / Xming / X natif ; sous Windows, **installation de VcXsrv en un clic** depuis Paramètres → Serveur X, via winget) |
| Outils | **générateur de clés SSH** (Ed25519, ECDSA, RSA ; enregistrement OpenSSH ou PuTTY `.ppk`, phrase secrète ; conversion d’une clé existante entre les deux formats), export / import de toute la configuration (sessions, dossiers, macros, tunnels, préférences ; jamais de mot de passe), recherche des mises à jour (menu Aide), scanner de ports (plages, sous-réseaux /24, listes prédéfinies, ouverture directe d'une session SSH / RDP / VNC / web sur un port trouvé), ports ouverts et connexions du poste (avec le programme), Wake-on-LAN (machines mémorisées) |
| Serveurs intégrés | HTTP (liste des dossiers, reprise de téléchargement), TFTP (blksize / tsize, PXE), FTP (compte ou anonyme), SSH/SFTP (mot de passe et/ou clés publiques autorisées, shell du poste, commandes, SFTP limité au dossier partagé), Telnet (shell du poste, non chiffré), Iperf (mesure de débit avec un client `iperf3`, TCP, normal / inverse / plusieurs flux), **VNC** (partage de l'écran de ce poste avec n'importe quelle visionneuse VNC : affichage seul par défaut ou prise de contrôle clavier / souris, mot de passe obligatoire ; Windows, et Linux sous X11) ; lecture seule possible, écoute locale ou réseau, journal d'activité en direct, démarrage avec Orbiterm |
| Terminal | transferts ZMODEM (`rz` sur le serveur : envoyer des fichiers, `sz` : recevoir ; Ctrl+C interrompt), écran partagé (2 côte à côte, 2 superposés, grille 2×2, mosaïque), MultiExec (saisie dans plusieurs terminaux), macros (enregistrer / rejouer, macro lancée au démarrage d’une session), recherche (Ctrl+Maj+F), mots-clés en couleur (erreurs, avertissements, succès), journalisation des sessions dans des fichiers texte (Documents/Orbiterm/journaux par défaut), commentaire de session (info-bulle), thèmes, zoom |
| Import | MobaXterm (`MobaXterm.ini`, exports `.moba`), PuTTY, OpenSSH `~/.ssh/config` — sans jamais importer de mot de passe |
| Apparence | interface épurée, **mode clair / sombre** (ou suivre le système, menu Affichage → Mode sombre), **thèmes d'interface** au choix (Affichage → Thème, ou Paramètres → Affichage) : crème, brun, **Cyberpunk néon**, **Gris sombre** et **Gris clair** (esthétique classique), avec les thèmes de terminal assortis ; palette brun profond / noisette / olive / ambre / crème, thèmes de terminal « Orbiterm brun » et « Orbiterm crème » |
| Configuration | fenêtre Paramètres : général, terminal, SSH, serveur X, affichage |

Disponible pour **Windows** (installateur) et **Linux** : Debian / Ubuntu (`.deb`), Fedora (`.rpm`) et NixOS (flake Nix).

---

## Tester l'application (recette)

### 1. Installer

| Système | Commande |
|---|---|
| **Windows** (winget) | `winget install Kaguyu-M.Orbiterm` |
| **Windows** (Scoop) | `scoop bucket add orbiterm https://github.com/Kaguyu-M/orbiterm` puis `scoop install orbiterm/orbiterm` |
| **Debian / Ubuntu** | voir ci-dessous (dépôt APT signé) |
| **Fedora** | voir ci-dessous (dépôt DNF signé) |
| **NixOS / Nix** | `nix profile install github:Kaguyu-M/orbiterm` |

Les fichiers (installateur Windows, version portable, .deb, .rpm) sont aussi sur la page
[Releases](https://github.com/Kaguyu-M/orbiterm/releases/latest). Page d’installation : https://kaguyu-m.github.io/orbiterm/

**Debian / Ubuntu** (Debian 13, Ubuntu 24.04 ou plus récent) :
```bash
sudo curl -fsSLo /usr/share/keyrings/orbiterm.asc https://kaguyu-m.github.io/orbiterm/orbiterm.asc
sudo curl -fsSLo /etc/apt/sources.list.d/orbiterm.sources https://kaguyu-m.github.io/orbiterm/orbiterm.sources
sudo apt update && sudo apt install orbiterm
```

**Fedora** (41 ou plus récent) :
```bash
sudo curl -fsSLo /etc/yum.repos.d/orbiterm.repo https://kaguyu-m.github.io/orbiterm/orbiterm.repo
sudo dnf install Orbiterm
```

**NixOS**, installation permanente dans le `flake.nix` du système :
```nix
inputs.orbiterm.url = "github:Kaguyu-M/orbiterm";
# puis dans la configuration :
imports = [ inputs.orbiterm.nixosModules.default ];
programs.orbiterm.enable = true;
```
(ou `environment.systemPackages = [ inputs.orbiterm.packages.x86_64-linux.default ];`, ou Home Manager : `home.packages`).
Essai sans installer : `nix run github:Kaguyu-M/orbiterm`.

Mises à jour : `winget upgrade`, `scoop update`, `apt upgrade`, `dnf upgrade`, `nix profile upgrade`.

> L'installateur Windows n'est pas signé : Windows SmartScreen peut afficher « Windows a protégé votre ordinateur ».
> Clique sur **Informations complémentaires → Exécuter quand même**.

> Sous Linux, les mots de passe mémorisés vont dans le trousseau du bureau (GNOME Keyring, KWallet, KeePassXC…
> via Secret Service). S'il est verrouillé, le système demande de le déverrouiller ; sans réponse, Orbiterm
> continue et redemande simplement le mot de passe.

> **Paramètres → Gestion des mots de passe…** liste les mots de passe qu'Orbiterm garde dans le trousseau
> (sessions, passerelles, VNC, serveurs intégrés) et permet de les remplacer ou de les supprimer ; leur valeur
> n'est jamais affichée.

### 2. Serveur de test (aucun serveur Linux nécessaire)
`livrables\serveur-de-test.exe` démarre un serveur SSH/SFTP sur `127.0.0.1:2222` (utilisateur `test`, mot de passe `test`)
avec un mini-shell (`help`, `echo`, `whoami`, `size`, `colors`, `xtest`, `exit`). Il accepte aussi les redirections de
ports et X11. Sa clé change à chaque démarrage : l'alerte « La clé du serveur a CHANGÉ » est alors normale.

- `livrables\serveur-ftp-de-test.exe` : FTP sur `127.0.0.1:2121` (`test` / `test` ou anonyme), dossier `%TEMP%\orbiterm-ftp-test`.
- `livrables\serveur-rdp-de-test.exe` : RDP sur `127.0.0.1:3390` (`test` / `test`, NLA) ; mire de couleurs, la touche `A` (position physique) peint le bas en jaune, un clic dessine un carré blanc, une tonalité de 440 Hz est jouée à la connexion. Option `--gateway 8444` : passerelle RD de test (HTTPS auto-signé, `gw` / `gwpass`).
- `livrables\serveur-vnc-de-test.exe` : VNC sur `127.0.0.1:5901` (mot de passe `test`) ; mire de couleurs, la touche `a` peint le bas en jaune, `b` en cyan, un carré blanc suit la souris.

### Liste de contrôle

| # | Vérification |
|---|---|
| 1 | **Session** → SSH 127.0.0.1:2222, utilisateur `test` → mot de passe, empreinte, terminal + onglet **Sftp** à gauche |
| 2 | Connexion rapide (en haut de la barre latérale) : `test@127.0.0.1:2222`, `powershell`, `cmd`, `wsl` |
| 3 | **Sessions → Importer des sessions…** : tes sessions MobaXterm apparaissent dans « Import MobaXterm » avec la même arborescence |
| 4 | **Split** : 2 côte à côte / grille 2×2 ; cliquer un onglet l'affiche dans le cadre sélectionné |
| 5 | **MultiExec** : taper une commande l'envoie à tous les terminaux ; décocher « saisie » exclut un terminal |
| 6 | Barre latérale **Macros** : Enregistrer → taper des commandes → Arrêter → nommer ; ▶ rejoue la macro |
| 7 | **Tunnels** : créer un tunnel local, le démarrer (LED verte), l'arrêter ; « Paramètres réseau » d'une session : rebond et redirections |
| 8 | **Paramètres** : curseur, copie à la sélection, keepalive SSH, démarrage auto du serveur X… |
| 9 | **Session → FTP** 127.0.0.1:2121 (`test`) puis **SFTP** 127.0.0.1:2222 : explorateur en plein onglet |
| 10 | **Session → VNC** 127.0.0.1:5901 → mot de passe `test` (cocher « Mémoriser ») → mire ; touche `a` = bas jaune |
| 11 | **Session → RDP** 127.0.0.1:3390 (`test` / `test`) puis une machine Windows de ton réseau : le bureau s'affiche **dans l'onglet** ; clavier, souris, molette, redimensionnement de la fenêtre, **Plein écran** (survoler le haut de l'écran pour le quitter) |
| 12 | **Session → Navigateur** : une URL (ex. `https://example.com`) ; ouvrir un menu masque la page le temps du menu |
| 13 | **Serveurs** : HTTP sur un dossier, Démarrer → ouvrir l'adresse affichée dans un navigateur ; journal mis à jour. FTP en anonyme, SSH avec un mot de passe (`ssh utilisateur@127.0.0.1`). Windows peut demander d'autoriser Orbiterm dans le pare-feu si « ce poste uniquement » est décoché |
| 14 | **Outils → Scanner de ports** sur ton réseau (`192.168.x.0/24`, « communs ») ; bouton SSH / RDP / Web sur un port trouvé. **Ports ouverts** : filtre par programme. **Wake-on-LAN** : adresse MAC d'une machine |
| 15 | Fermer / rouvrir : sessions, tunnels, macros, serveurs et réglages conservés |

Les données sont dans `%APPDATA%\Orbiterm\Orbiterm\config\` (`sessions.toml`, `known_hosts`) ;
les mots de passe dans le **Gestionnaire d'identifiants Windows** (entrées `…orbiterm`).

---

## Développement

Prérequis : Rust (stable), Node.js 20+, sous Windows les Build Tools C++ de Visual Studio,
sous Linux (Debian) `build-essential pkg-config libwebkit2gtk-4.1-dev libssl-dev libdbus-1-dev libxdo-dev libayatana-appindicator3-dev librsvg2-dev` (voir https://tauri.app/start/prerequisites/).

```bash
npm install
npm run tauri dev            # lancer en mode développement
cd src-tauri && cargo test   # tests unitaires + bout en bout (SSH, SFTP, FTP, VNC, RDP, tunnels, rebond, X11, PTY, Telnet, import, serveurs intégrés, scanner)
npm run check                # vérification TypeScript/Svelte
npm run tauri build          # installateurs : .exe (NSIS) sous Windows, .deb + .rpm sous Linux
nix build path:.             # paquet Nix (NixOS), résultat dans ./result
cargo run --example test_server -- 2222   # serveur SSH/SFTP de test (user test / test)
cargo run --example ftp_test_server        # FTP de test sur 2121
cargo run --example vnc_test_server        # VNC de test sur 5901 (mot de passe test)
cargo run --example builtin_server -- iperf 5201   # un serveur intégré seul (http, tftp, ftp, ssh, telnet, iperf ; mot de passe : ORB_SERVER_PW)
cd tools/rdp-test-server && cargo run       # RDP de test sur 3390 (test / test) — utilisé par tests/rdp.rs
```

### Structure

```
src/                         interface (Svelte 5 + xterm.js)
  routes/+page.svelte        fenêtre principale : menus, barre d'outils, onglets, écran partagé, dialogues
  lib/app.svelte.ts          état global (onglets, split, MultiExec, macros, préférences)
  lib/Terminal.svelte        terminal xterm.js relié à une connexion
  lib/Sidebar.svelte         sessions / outils / macros / SFTP
  lib/SessionDialog.svelte   paramètres d'une session
  lib/TunnelManager.svelte   gestionnaire de tunnels
  lib/SettingsDialog.svelte  configuration générale
  lib/ImportDialog.svelte    import MobaXterm / PuTTY / OpenSSH
  lib/EditorView.svelte      onglet éditeur de texte (CodeMirror)
  lib/DiffView.svelte        onglet de comparaison de fichiers / dossiers
  lib/update.ts              recherche des mises à jour (publications GitHub)
  lib/zmodem.ts              transferts ZMODEM dans le terminal (zmodem.js)
  lib/highlight.ts           mots-clés en couleur, suivi des lignes du terminal
  lib/VncView.svelte         visionneuse VNC (noVNC)
  lib/RdpView.svelte         bureau RDP dans l'onglet (canvas, clavier par scancodes)
  lib/ExternalView.svelte    onglet d'une session RDP ouverte dans une fenêtre séparée (mstsc)
  lib/ServersDialog.svelte   fenêtre des serveurs intégrés
  lib/PortScanner.svelte, OpenPorts.svelte, WakeOnLan.svelte  outils réseau
  lib/BrowserView.svelte     onglet navigateur (webview native enfant)
src-tauri/src/               backend Rust
  ssh.rs, forward.rs, x11.rs SSH (russh), redirections, X11
  pty.rs, telnet.rs, serial.rs  terminaux locaux, Telnet/Rlogin, port série
  sftp.rs, known_hosts.rs    SFTP, clés d'hôte
  scp.rs                     navigateur de fichiers sans SFTP (commandes du shell + SCP)
  editor.rs                  fichiers texte de l’éditeur (encodage, fins de ligne)
  dircmp.rs, bundle.rs       comparaison de dossiers, export / import de la configuration
  termlog.rs                 journaux des sessions (fichiers texte)
  config.rs, secrets.rs      sessions.toml (écriture atomique), trousseau du système
  importer.rs                import de sessions
  rdp_client.rs              client RDP intégré (IronRDP) + pont WebSocket vers l'onglet
  rdp_channels.rs            canaux RDP : presse-papiers (cliprdr) et dossier partagé (rdpdr)
  ftp.rs, rdp.rs, vnc.rs     FTP/FTPS (suppaftp), RDP externe (mstsc / FreeRDP), pont WebSocket ↔ VNC
  browser.rs                 webviews enfants du navigateur
  servers/                   serveurs intégrés HTTP, TFTP, FTP (libunftp), SSH (russh), Telnet, Iperf, VNC (vncd.rs + screen.rs : capture et saisie GDI / X11) ; sftp_server.rs
  nettools.rs                scanner de ports, sockets du poste, Wake-on-LAN
  commands.rs, tools_commands.rs  API exposée à l'interface
src-tauri/examples/                 serveurs de test SSH/SFTP, FTP, VNC ; lancement d'un serveur intégré seul
tools/rdp-test-server/              serveur RDP de test (projet Cargo séparé)
src-tauri/tests/                    tests de bout en bout
```

## Licence

MIT — voir [LICENSE](LICENSE). Les bibliothèques utilisées gardent leur propre licence (par exemple zmodem.js : Apache-2.0, xterm.js et CodeMirror : MIT).
