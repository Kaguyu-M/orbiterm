# Orbiterm

Terminal multi-protocoles multiplateforme (Windows + Linux) : onglets, sessions enregistrées, navigateur SFTP, tunnels SSH,
X11, RDP, VNC, FTP, navigateur web, écran partagé, MultiExec et macros. Application originale dont l'ergonomie s'inspire de MobaXterm.
Conception : [`docs/CONCEPTION.md`](docs/CONCEPTION.md).

**Version 0.5.0**

| Domaine | Fonctionnalités |
|---|---|
| Sessions | SSH, Telnet, Rlogin, port série, Mosh, shells locaux (PowerShell, cmd, Git Bash…), WSL ; dossiers, glisser-déposer, recherche, connexion rapide, sessions récentes |
| SSH | mot de passe (trousseau du système) ou clé, vérification des clés d'hôte, navigateur SFTP, compression, commande distante, keepalive |
| Bureaux distants | RDP (Windows : client mstsc dans sa fenêtre, taille / presse-papiers / lecteurs / imprimantes / son / admin ; Linux : FreeRDP), VNC intégré dans l'onglet (noVNC : mot de passe mémorisable, ajustement, lecture seule, Ctrl+Alt+Suppr, presse-papiers) |
| Fichiers | onglets SFTP et FTP / FTPS autonomes (explorateur plein onglet avec colonnes, envoi / téléchargement / glisser-déposer) |
| Navigateur | onglet web intégré (précédent / suivant / actualiser / barre d'adresse), sans aucun accès aux commandes d'Orbiterm |
| Réseau | redirections -L / -R / -D (SOCKS) par session, gestionnaire de tunnels (démarrage auto), rebond SSH (jump host, aussi pour RDP / VNC), redirection X11 (VcXsrv / Xming / X natif) |
| Terminal | écran partagé (2 côte à côte, 2 superposés, grille 2×2, mosaïque), MultiExec (saisie dans plusieurs terminaux), macros (enregistrer / rejouer), thèmes, zoom |
| Import | MobaXterm (`MobaXterm.ini`, exports `.moba`), PuTTY, OpenSSH `~/.ssh/config` — sans jamais importer de mot de passe |
| Configuration | fenêtre Paramètres : général, terminal, SSH, serveur X, affichage |

Prochaines étapes : outils réseau (scanner de ports, Wake-on-LAN) et serveurs
intégrés (HTTP, TFTP, FTP, SSH) ; version Linux.

---

## Tester l'application (recette)

### 1. Installer
Lance `livrables\Orbiterm_0.5.0_x64-setup.exe`.

> L'installateur n'est pas signé : Windows SmartScreen peut afficher « Windows a protégé votre ordinateur ».
> Clique sur **Informations complémentaires → Exécuter quand même**.

### 2. Serveur de test (aucun serveur Linux nécessaire)
`livrables\serveur-de-test.exe` démarre un serveur SSH/SFTP sur `127.0.0.1:2222` (utilisateur `test`, mot de passe `test`)
avec un mini-shell (`help`, `echo`, `whoami`, `size`, `colors`, `xtest`, `exit`). Il accepte aussi les redirections de
ports et X11. Sa clé change à chaque démarrage : l'alerte « La clé du serveur a CHANGÉ » est alors normale.

- `livrables\serveur-ftp-de-test.exe` : FTP sur `127.0.0.1:2121` (`test` / `test` ou anonyme), dossier `%TEMP%\orbiterm-ftp-test`.
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
| 11 | **Session → RDP** vers une machine de ton réseau : mstsc s'ouvre ; fermer l'onglet ferme la fenêtre |
| 12 | **Session → Navigateur** : une URL (ex. `https://example.com`) ; ouvrir un menu masque la page le temps du menu |
| 13 | Fermer / rouvrir : sessions, tunnels, macros et réglages conservés |

Les données sont dans `%APPDATA%\Orbiterm\Orbiterm\config\` (`sessions.toml`, `known_hosts`) ;
les mots de passe dans le **Gestionnaire d'identifiants Windows** (entrées `…orbiterm`).

---

## Développement

Prérequis : Rust (stable), Node.js 20+, sous Windows les Build Tools C++ de Visual Studio,
sous Linux `libwebkit2gtk-4.1-dev libssl-dev libsecret-1-dev libudev-dev` (voir https://tauri.app/start/prerequisites/).

```bash
npm install
npm run tauri dev            # lancer en mode développement
cd src-tauri && cargo test   # tests unitaires + bout en bout (SSH, SFTP, FTP, VNC, tunnels, rebond, X11, PTY, Telnet, import)
npm run check                # vérification TypeScript/Svelte
npm run tauri build          # installateurs (.msi/.exe sous Windows, .deb/.AppImage sous Linux)
cargo run --example test_server -- 2222   # serveur SSH/SFTP de test (user test / test)
cargo run --example ftp_test_server        # FTP de test sur 2121
cargo run --example vnc_test_server        # VNC de test sur 5901 (mot de passe test)
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
  lib/VncView.svelte         visionneuse VNC (noVNC)
  lib/ExternalView.svelte    onglet d'une session RDP ouverte dans sa fenêtre
  lib/BrowserView.svelte     onglet navigateur (webview native enfant)
src-tauri/src/               backend Rust
  ssh.rs, forward.rs, x11.rs SSH (russh), redirections, X11
  pty.rs, telnet.rs, serial.rs  terminaux locaux, Telnet/Rlogin, port série
  sftp.rs, known_hosts.rs    SFTP, clés d'hôte
  config.rs, secrets.rs      sessions.toml (écriture atomique), trousseau du système
  importer.rs                import de sessions
  ftp.rs, rdp.rs, vnc.rs     FTP/FTPS (suppaftp), RDP (mstsc / FreeRDP), pont WebSocket ↔ VNC
  browser.rs                 webviews enfants du navigateur
  commands.rs                API exposée à l'interface
src-tauri/examples/                 serveurs de test SSH/SFTP, FTP, VNC
src-tauri/tests/                    tests de bout en bout
```
