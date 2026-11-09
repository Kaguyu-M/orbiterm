# Orbiterm

Client SSH multiplateforme (Windows + Linux) avec onglets de terminal et navigateur SFTP intégré.
Application originale inspirée des fonctionnalités de MobaXterm. Conception : [`docs/CONCEPTION.md`](docs/CONCEPTION.md).

**Version 0.1.0 (MVP)** : sessions SSH (dossiers, recherche), terminal en onglets, mots de passe dans le trousseau
du système, vérification des clés d'hôte, navigateur SFTP (navigation, envoi, téléchargement, renommage, suppression).

---

## Tester l'application (recette)

### 1. Installer
Lance `livrables\Orbiterm_0.1.0_x64-setup.exe`.

> L'installateur n'est pas signé : Windows SmartScreen peut afficher « Windows a protégé votre ordinateur ».
> Clique sur **Informations complémentaires → Exécuter quand même**.

### 2. Option A : avec le serveur de test fourni (aucun serveur Linux nécessaire)
1. Lance `livrables\serveur-de-test.exe` (une console s'ouvre et affiche le port **2222** et l'empreinte de sa clé).
   Windows peut demander l'autorisation du pare-feu : il n'écoute que sur `127.0.0.1`, tu peux refuser l'accès réseau.
2. Dans Orbiterm : **＋ Session** → Hôte `127.0.0.1`, Port `2222`, Utilisateur `test` → Enregistrer.
3. Double-clique la session → mot de passe `test`.
4. Le serveur de test fournit un mini-shell (`help`, `echo`, `size`, `colors`, `exit`) et un vrai SFTP
   dans `%TEMP%\orbiterm-test-server`.

> Le serveur de test génère une nouvelle clé à chaque démarrage : à la 2ᵉ exécution, Orbiterm affichera
> l'alerte **« La clé du serveur a CHANGÉ »**. C'est le comportement attendu (et une bonne occasion de la voir).

### 3. Option B : avec un vrai serveur (VPS, Raspberry Pi, NAS…)
Crée une session avec ses informations habituelles (mot de passe ou clé privée OpenSSH).
Là tu as un vrai shell : essaie `htop`, `vim`, `ls --color`, redimensionne la fenêtre.

### Liste de contrôle

| # | Vérification |
|---|---|
| 1 | Créer un dossier, une session dedans, la modifier, la dupliquer, la rechercher |
| 2 | Fermer / rouvrir l'application : sessions conservées |
| 3 | 1ʳᵉ connexion : la fenêtre « Serveur inconnu » montre l'empreinte ; « Faire confiance » |
| 4 | Mauvais mot de passe : redemandé avec « ⚠ Mot de passe refusé » |
| 5 | Mot de passe mémorisé : la connexion suivante ne demande plus rien |
| 6 | Terminal : couleurs, copier (Ctrl+Maj+C) / coller (Ctrl+Maj+V), redimensionnement, plusieurs onglets (Ctrl+Tab) |
| 7 | Panneau SFTP (ouvert automatiquement) : naviguer, créer un dossier, **Envoyer…**, glisser-déposer des fichiers, télécharger (double-clic ou ⬇), renommer (F2), supprimer (Suppr) |
| 8 | Taper `exit` : bouton **Reconnecter** |
| 9 | Supprimer une session : son mot de passe est retiré du gestionnaire d'identifiants Windows |

Les données sont dans `%APPDATA%\Orbiterm\Orbiterm\config\` (`sessions.toml`, `known_hosts`) ;
les mots de passe dans le **Gestionnaire d'identifiants Windows** (entrées `…orbiterm`).

---

## Développement

Prérequis : Rust (stable), Node.js 20+, sous Windows les Build Tools C++ de Visual Studio,
sous Linux `libwebkit2gtk-4.1-dev libssl-dev libsecret-1-dev` (voir https://tauri.app/start/prerequisites/).

```bash
npm install
npm run tauri dev            # lancer en mode développement
cd src-tauri && cargo test   # tests unitaires + tests de bout en bout SSH/SFTP
npm run check                # vérification TypeScript/Svelte
npm run tauri build          # installateurs (.msi/.exe sous Windows, .deb/.AppImage sous Linux)
cargo run --example test_server -- 2222   # serveur SSH/SFTP de test (user test / test)
```

### Structure

```
src/                      interface (Svelte 5 + xterm.js)
  routes/+page.svelte     fenêtre principale : barre latérale, onglets, dialogues
  lib/Terminal.svelte     terminal xterm.js relié à une connexion
  lib/SftpPanel.svelte    navigateur de fichiers
  lib/SessionTree.svelte  arborescence des sessions
  lib/api.ts, bus.ts      appels au backend / routage des événements
src-tauri/src/            backend Rust
  ssh.rs                  connexion SSH (russh), authentification, canal shell + canal SFTP
  sftp.rs                 opérations SFTP
  known_hosts.rs          vérification des clés d'hôte (format OpenSSH)
  config.rs               sessions.toml (écriture atomique)
  secrets.rs              trousseau du système (keyring)
  commands.rs             API exposée à l'interface
src-tauri/examples/test_server.rs   serveur SSH/SFTP de test
src-tauri/tests/e2e.rs              tests de bout en bout
```
