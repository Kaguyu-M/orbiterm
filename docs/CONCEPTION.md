# Orbiterm – Document de conception (MVP)

> Nom de code provisoire : **Orbiterm** (renommable). Application originale inspirée des *fonctionnalités* de MobaXterm
> (voir `..\..\MobaXterm-Analyse\RAPPORT_ANALYSE_MOBAXTERM.md`), multiplateforme **Windows + Linux**.

## 1. Objectifs du MVP

| # | Fonction | Critère d'acceptation |
|---|---|---|
| F1 | **Gestionnaire de sessions SSH** | Créer, modifier, supprimer et dupliquer des sessions ; les ranger en dossiers ; tout est persistant entre deux lancements. |
| F2 | **Onglets de terminal SSH** | Double-clic sur une session = nouvel onglet avec un terminal complet (couleurs 256 / truecolor, vim, htop, redimensionnement, copier-coller). Plusieurs onglets en parallèle. |
| F3 | **Authentification** | Mot de passe, clé privée (OpenSSH, avec ou sans passphrase), keyboard-interactive. Option « mémoriser le mot de passe » dans le trousseau du système. |
| F4 | **Vérification de la clé d'hôte** | Une clé inconnue est demandée à l'utilisateur ; une clé modifiée déclenche une alerte bloquante. Stockage dans un `known_hosts` compatible OpenSSH. |
| F5 | **Navigateur SFTP** | Panneau latéral ouvert automatiquement avec la session SSH : naviguer, envoyer, télécharger (glisser-déposer + boutons), créer un dossier, renommer, supprimer, afficher les fichiers cachés, rafraîchir. Barre de progression. |
| F6 | **Multiplateforme** | Builds Windows (.msi/.exe) et Linux (.AppImage/.deb) depuis le même code. |

Hors MVP (versions suivantes) : shell local (ConPTY/openpty), tunnels et jump hosts, X11 forwarding, split/MultiExec,
macros, monitoring distant, import de MobaXterm.ini / PuTTY, RDP/VNC, serveurs intégrés.

## 2. Pile technique

| Couche | Choix | Pourquoi |
|---|---|---|
| Application | **Tauri 2** | Binaire léger, webview système (WebView2 sous Windows, WebKitGTK sous Linux) |
| Backend | **Rust** + `tokio` | Asynchrone, sûr, multiplateforme |
| SSH | `russh` (+ `russh-keys`) | Client SSH en pur Rust, sans dépendance C, compile pareil partout |
| SFTP | `russh-sftp` | SFTP sur un **canal** de la même connexion SSH (pas de seconde connexion, contrairement à MobaXterm) |
| Secrets | `keyring` | Windows Credential Manager / Secret Service (libsecret) |
| Config | `serde` + **TOML** (`toml`) | Lisible et versionnable |
| Chemins | `directories` | `%APPDATA%\Orbiterm` / `~/.config/orbiterm` |
| UI | **TypeScript + Svelte 5 + Vite** | Léger, réactif, peu de code |
| Terminal | **xterm.js** (+ addons fit, web-links, unicode11) | Émulateur éprouvé (utilisé par VS Code) |
| Tests | `cargo test` (unitaires + intégration contre un serveur SSH de test), `vitest` (UI) | |

## 3. Architecture

```
┌────────────────────────── UI (webview, Svelte) ───────────────────────────┐
│ Sidebar[Sessions | SFTP]   TabBar   TerminalView(xterm.js)   Dialogs       │
│        │ invoke("cmd", …)                ▲ events "term://data/<id>" …     │
└────────┼─────────────────────────────────┼────────────────────────────────┘
         ▼                                 │
┌──────────────────────── Backend Rust (Tauri) ─────────────────────────────┐
│ commands.rs ── SessionStore (TOML)    SecretStore (keyring)               │
│            ── ConnectionManager: HashMap<ConnId, SshConnection>           │
│                   SshConnection = russh client Handle                     │
│                     ├─ canal "shell" + PTY  → flux vers l'UI              │
│                     └─ canal "sftp" (paresseux) → SftpSession             │
│            ── KnownHosts (fichier OpenSSH)  HostKeyPrompt (aller-retour UI)│
└───────────────────────────────────────────────────────────────────────────┘
```

Principe clé (amélioration par rapport à MobaXterm) : **une seule connexion SSH par onglet, multiplexée**.
Le terminal et le SFTP sont deux canaux de cette connexion, donc une seule authentification.
Les tunnels et le monitoring viendront plus tard comme canaux supplémentaires.

### 3.1 Modules Rust (`src-tauri/src/`)

| Module | Rôle |
|---|---|
| `main.rs` / `lib.rs` | Démarrage Tauri, enregistrement des commandes, état partagé |
| `config/sessions.rs` | Modèle `Session`, `Folder` ; chargement et sauvegarde atomiques (`sessions.toml.tmp` → rename) |
| `secrets.rs` | `get/set/delete_password(session_id)` via keyring (service `orbiterm`) |
| `ssh/connection.rs` | Connexion, authentification, ouverture du canal shell + PTY, boucle de lecture → événements |
| `ssh/known_hosts.rs` | Lecture et écriture du fichier `known_hosts` ; vérification : Inconnue / Correspond / Modifiée |
| `ssh/auth.rs` | Chargement des clés, passphrase, keyboard-interactive |
| `sftp.rs` | list, stat, mkdir, rename, remove(rec), upload/download en flux avec progression |
| `commands.rs` | Façade `#[tauri::command]` (voir §4) |
| `error.rs` | Type d'erreur unique sérialisable vers l'UI |

### 3.2 Modèle de données (`sessions.toml`)

```toml
version = 1

[[folders]]
id = "f-…"            # UUID
name = "Production"
parent = ""           # "" = racine

[[sessions]]
id = "s-…"
name = "web-01"
folder = "f-…"
kind = "ssh"          # extensible : "local", "telnet", "serial"…
host = "10.0.0.5"
port = 22
username = "admin"
auth = "password"     # "password" | "key" | "agent" (plus tard)
key_path = ""         # si auth = "key"
remember_password = true   # le mot de passe lui-même est dans le trousseau, JAMAIS dans le TOML
sftp_follow_cwd = false
[sessions.terminal]
font_family = "Cascadia Mono, DejaVu Sans Mono, monospace"
font_size = 13
scrollback = 10000
theme = "dark"
```

Fichiers : `<config>/sessions.toml`, `<config>/known_hosts`, `<config>/settings.toml`.
Mode portable plus tard : un dossier `data/` à côté de l'exécutable remplace `<config>`.

## 4. Contrat IPC (UI ↔ backend)

**Commandes (`invoke`)**

| Commande | Entrée | Sortie |
|---|---|---|
| `sessions_load` | – | `{folders, sessions}` |
| `session_save` / `session_delete` | `Session` / `id` | – |
| `folder_save` / `folder_delete` | `Folder` / `id` | – |
| `ssh_connect` | `session_id, cols, rows, password?, passphrase?, remember?` | `conn_id` |
| `ssh_write` | `conn_id, data (bytes)` | – |
| `ssh_resize` | `conn_id, cols, rows` | – |
| `ssh_disconnect` | `conn_id` | – |
| `hostkey_answer` | `request_id, accept: bool` | – |
| `sftp_list` | `conn_id, path` | `[Entry{name, kind, size, mtime, perms}]` |
| `sftp_home` | `conn_id` | `path` |
| `sftp_mkdir` / `sftp_rename` / `sftp_remove` | … | – |
| `sftp_download` / `sftp_upload` | `conn_id, remote, local, transfer_id` | – (progression par événement) |

**Événements (backend → UI)**

| Événement | Charge utile |
|---|---|
| `term-data` | `{conn_id, data}` |
| `term-closed` | `{conn_id, reason}` |
| `hostkey-prompt` | `{request_id, host, port, algo, fingerprint, status: "unknown" \| "changed"}` |
| `auth-prompt` | `{conn_id, prompts[]}` (keyboard-interactive) |
| `transfer-progress` | `{transfer_id, done, total}` |

## 5. Sécurité

- Les mots de passe ne sont jamais écrits dans un fichier ni dans les logs. Ils restent dans le trousseau du système, ou en mémoire le temps de la connexion.
- Une clé d'hôte modifiée bloque la connexion, avec un message explicite. Une clé inconnue est montrée avec son empreinte SHA-256.
- Tauri : CSP stricte et capabilities minimales (pas d'accès shell ni fs générique depuis l'UI ; les fichiers passent uniquement par les commandes SFTP et la boîte de dialogue native).
- Chemins : les noms renvoyés par le serveur sont validés avant d'écrire en local (pas de `..`, pas de chemin absolu).

## 6. Plan de tests

1. **Unitaires** : sérialisation des sessions (aller-retour TOML), `known_hosts` (inconnue / correspond / modifiée), validation des chemins SFTP.
2. **Intégration** contre un vrai serveur SSH de test (OpenSSH dans WSL ou en conteneur, sinon serveur `russh` embarqué dans les tests) :
   connexion par mot de passe et par clé, shell (écriture `echo` → lecture), redimensionnement,
   SFTP (mkdir → upload → list → download → comparaison du contenu → rename → remove).
3. **Build** : compilation release Windows. Linux via WSL ou CI GitHub Actions (matrice windows-latest / ubuntu-latest).
4. **Recette utilisateur** (toi) : liste de contrôle à chaque jalon.

## 7. Jalons

| Jalon | Contenu | Livré quand |
|---|---|---|
| J0 | Outils installés, squelette Tauri qui démarre | fenêtre vide qui s'ouvre |
| J1 | Gestionnaire de sessions (UI + TOML) | F1 OK |
| J2 | Terminal SSH en onglets + known_hosts + trousseau | F2, F3, F4 OK |
| J3 | Navigateur SFTP | F5 OK |
| J4 | Packaging Windows (+ Linux) et finitions | F6 OK → recette utilisateur |
