# Orbiterm — consignes pour Claude Code

Terminal multi-protocoles Windows + Linux (Tauri 2 + Rust, SvelteKit / Svelte 5, xterm.js), ergonomie inspirée
de MobaXterm. Fonctionnalités : `README.md` ; écarts avec MobaXterm : `docs/COMPARAISON_MOBAXTERM.md` ;
conception : `docs/CONCEPTION.md`.

## Langue et style

- Répondre à l'utilisateur **en français**. Textes de l'interface, messages de commit et docs : français.
  Commentaires du code : anglais.
- Commits : auteur `Kaguyu-M <213601983+Kaguyu-M@users.noreply.github.com>` (adresse noreply : ne jamais
  utiliser d'autre adresse), passé par `git -c user.name=… -c user.email=…` ou la config locale du dépôt.
  Message terminé par la ligne `Co-Authored-By:` de Claude.
- Ne pas modifier les thèmes crème et brun. Toute vue distante (RDP, VNC, X11…) reste **intégrée dans
  l'onglet**, jamais dans une fenêtre flottante (une fenêtre externe seulement en option explicite).
- Paquets : .deb, .rpm, pacman (Arch / Omarchy), Nix, Scoop, winget. **Pas d'AppImage.**

## Commandes

```bash
npm ci
npm run check                     # svelte-check (exigé par la CI)
npm run build                     # frontend
cd src-tauri && cargo test --locked   # tests unitaires + bout en bout (exigés par la CI)
npx tauri build --no-bundle       # binaire release (src-tauri/target/release/orbiterm)
cd src-tauri && cargo test --test exp_xxx -- --ignored --nocapture   # expériences contre de vraies machines
```
- Les tests contre de vraies machines (`#[ignore]`) lisent hôte / compte / mot de passe dans des variables
  d'environnement (`ORB_*`), jamais dans le code. Les fichiers d'expérience temporaires (`tests/exp_*.rs`) ne
  sont jamais commités.
- Supprimer `src-tauri/target/debug` après un gros chantier (il grossit de dizaines de Go).
- Journal de diagnostic de l'appli : variable `ORBITERM_LOG` (filtre env_logger, ex.
  `orbiterm_lib=debug,ironrdp_connector=info`), écrit sur stderr.

## Règles de code

- Jamais de glisser-déposer HTML5 (`draggable` / `ondrop`) : le gestionnaire de dépôt natif de Tauri le
  désactive sous Windows. Utiliser les événements pointeur (voir `SessionTree.svelte`, `FilesView.svelte`).
- Chaque nouvel appel de plugin Tauri (dialogues, presse-papiers…) : vérifier la permission dans
  `src-tauri/capabilities/default.json` (un crochet de test qui contourne le dialogue cache l'oubli).
- Crates IronRDP modifiées dans `src-tauri/vendor/` via `[patch.crates-io]` : chaque modification porte le
  repère `Orbiterm patch`. À retirer quand l'amont corrige.
- Ajout ou mise à jour d'une dépendance npm : le hash npm de Nix (`nix/package.nix`) change ; le workflow
  `nix.yml` le corrige tout seul sur `main` par un commit « Nix : hash des dépendances npm ».
- Mots de passe : uniquement dans le trousseau du système (Gestionnaire d'identifiants Windows, Secret Service
  sous Linux). Jamais dans un fichier, une ligne de commande, un export, un journal ou un commit.

## Publication d'une version

1. Passer la version dans **5 fichiers** : `package.json`, `package-lock.json` (2 occurrences),
   `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock` (paquet `orbiterm`), `src-tauri/tauri.conf.json`.
   Commit « Version X.Y.Z », push sur `main`.
2. Attendre les workflows « Vérifications » et « Nix » sur ce commit. « Nix » pousse ensuite le commit
   « Nix : hash des dépendances npm » : faire `git pull`.
3. Poser l'étiquette `vX.Y.Z` **sur ce commit Nix** (sinon le paquet Nix de l'étiquette ne compile pas) et la
   pousser : `.github/workflows/release.yml` repasse les vérifications, compile (NSIS + zip portable sous
   Windows, .deb + .rpm sous Ubuntu), crée la Release GitHub, construit les dépôts APT / DNF / pacman signés
   (GitHub Pages) et le manifeste Scoop (le bot commite `bucket/orbiterm.json` sur `main` : `git pull` après).
4. Jamais deux publications en même temps. Si un job échoue pour une raison passagère :
   `gh run rerun <id> --failed`.
- winget : PR microsoft/winget-pkgs en attente de modération (paquet `Kaguyu-M.Orbiterm`) ; ensuite
  `wingetcreate update` pour chaque version. AUR : l'étape existe dans `release.yml`, inactive tant que le
  secret `AUR_SSH_KEY` n'existe pas.

## Tests d'interface

- Ne jamais injecter clavier / souris sur l'écran réel de l'utilisateur sans son accord explicite pour ce
  test. Sous Linux, utiliser un Xvfb privé (`:77`) + xdotool.
- Un test qui touche au presse-papiers le sauvegarde, le restaure et vérifie la restauration.
- Un test nettoie ce qu'il crée : sessions, entrées du trousseau, fichiers de config.
