# Orbiterm built from source (Tauri 2: SvelteKit front end + Rust back end).
{
  lib,
  rustPlatform,
  fetchNpmDeps,
  cargo-tauri,
  nodejs,
  npmHooks,
  pkg-config,
  wrapGAppsHook3,
  openssl,
  webkitgtk_4_1,
  glib-networking,
  dbus,
  aws-lc,
}:

let
  root = ../.;
  conf = lib.importJSON ../src-tauri/tauri.conf.json;
in
rustPlatform.buildRustPackage (finalAttrs: {
  pname = "orbiterm";
  inherit (conf) version;

  src = lib.fileset.toSource {
    inherit root;
    fileset = lib.fileset.difference (lib.fileset.unions [
      ../package.json
      ../package-lock.json
      ../svelte.config.js
      ../vite.config.js
      ../tsconfig.json
      ../src
      ../static
      ../src-tauri
    ]) (lib.fileset.maybeMissing ../src-tauri/target);
  };

  npmDeps = fetchNpmDeps {
    name = "${finalAttrs.pname}-${finalAttrs.version}-npm-deps";
    inherit (finalAttrs) src;
    hash = "sha256-aeyj1BIbRXpRdZ7x92HPKC6m3sV6GZ0q6VQ1045XkPM=";
  };

  cargoRoot = "src-tauri";
  buildAndTestSubdir = finalAttrs.cargoRoot;
  cargoLock.lockFile = ../src-tauri/Cargo.lock;

  nativeBuildInputs = [
    cargo-tauri.hook
    nodejs
    npmHooks.npmConfigHook
    pkg-config
    wrapGAppsHook3
  ];

  buildInputs = [
    openssl
    webkitgtk_4_1
    glib-networking # TLS for the embedded web views
    dbus # secret service (passwords in the system keyring)
    aws-lc # rustls crypto (built-in FTP server); nixpkgs links the system AWS-LC
  ];

  # the test suite opens network servers and uses the session keyring
  doCheck = false;

  meta = {
    description = "Terminal et client d'accès distant tout-en-un (SSH, SFTP, RDP, VNC, Telnet, série…)";
    license = lib.licenses.mit;
    mainProgram = "orbiterm";
    platforms = lib.platforms.linux;
  };
})
