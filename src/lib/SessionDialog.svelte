<script lang="ts">
  import { onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import Modal from "./Modal.svelte";
  import Icon from "./Icon.svelte";
  import { app } from "./app.svelte";
  import { api, IS_WINDOWS, newForward, PROTOCOLS, protocol, viewOf, type Session } from "./api";

  let { session }: { session: Session } = $props();

  // edit a copy, so "Annuler" really cancels
  // svelte-ignore state_referenced_locally
  const initial = structuredClone($state.snapshot(session)) as Session;
  initial.forwards ??= [];
  let s: Session = $state(initial);
  // svelte-ignore state_referenced_locally
  const isNew = !session.id;
  let error = $state("");
  let bottom: "advanced" | "terminal" | "network" | "bookmark" = $state("advanced");
  let ports: string[] = $state([]);
  // svelte-ignore state_referenced_locally
  let specifyUser = $state(!!session.username || session.kind === "ssh");

  const p = $derived(protocol(s.kind));
  const isRemote = $derived(["ssh", "telnet", "rlogin", "mosh", "rdp", "vnc", "ftp", "sftp"].includes(s.kind));
  const hasUser = $derived(s.kind !== "vnc");
  const sshLike = $derived(s.kind === "ssh" || s.kind === "sftp");
  const hasGateway = $derived(["ssh", "sftp", "rdp", "vnc"].includes(s.kind));
  const RDP_SIZES = ["1024x768", "1280x800", "1366x768", "1600x900", "1920x1080", "2560x1440"];
  const wslDistros = $derived(app.shells.filter((x) => x.id.startsWith("wsl:")).map((x) => x.id.slice(4)));
  const localShells = $derived(app.shells.filter((x) => !x.id.startsWith("wsl:")));

  function x(key: string, fallback = ""): string {
    return s.extra[key] ?? fallback;
  }
  function setX(key: string, v: string) {
    s.extra[key] = v;
  }

  function pickKind(kind: string) {
    const prev = protocol(s.kind);
    // keep a custom port, switch a default one
    if (!s.port || s.port === prev.port) s.port = protocol(kind).port;
    s.kind = kind;
    if (kind === "serial") refreshPorts();
  }

  async function refreshPorts() {
    ports = await api.serialPorts().catch(() => []);
    if (!x("serial_port") && ports.length) setX("serial_port", ports[0]);
  }

  onMount(() => {
    if (s.kind === "serial") refreshPorts();
  });

  function folderPath(id: string): string {
    const parts: string[] = [];
    let cur = app.store.folders.find((f) => f.id === id);
    while (cur) {
      parts.unshift(cur.name);
      cur = app.store.folders.find((f) => f.id === cur!.parent);
    }
    return parts.join(" / ");
  }

  async function pickKey() {
    const f = await open({ multiple: false, directory: false, title: "Clé privée SSH" });
    if (typeof f === "string") s.key_path = f;
  }
  async function pickJumpKey() {
    const f = await open({ multiple: false, directory: false, title: "Clé privée du rebond SSH" });
    if (typeof f === "string") setX("jump_key", f);
  }
  async function pickDir() {
    const d = await open({ directory: true, title: "Répertoire de démarrage" });
    if (typeof d === "string") setX("cwd", d);
  }
  async function pickShare() {
    const d = await open({ directory: true, title: "Dossier local partagé avec le poste distant" });
    if (typeof d === "string") setX("rdp_share_dir", d);
  }
  async function pickProgram() {
    const f = await open({ multiple: false, directory: false, title: "Programme à lancer" });
    if (typeof f === "string") setX("program", f);
  }

  function defaultName(): string {
    switch (s.kind) {
      case "shell":
        return localShells.find((l) => l.id === x("shell"))?.label ?? (x("program") || "Terminal local");
      case "wsl":
        return `WSL ${x("distro") || ""}`.trim();
      case "serial":
        return `${x("serial_port") || "Série"} (${x("baud", "115200")})`;
      default:
        return s.username ? `${s.username}@${s.host}` : s.host;
    }
  }

  function ok() {
    error = "";
    if (!p.ready) return (error = `Le type « ${p.label} » n'est pas encore disponible.`);
    s.host = s.host.trim();
    s.username = specifyUser ? s.username.trim() : "";
    if (isRemote) {
      if (!s.host) return (error = "L'hôte distant est obligatoire.");
      if (!s.port || s.port < 1 || s.port > 65535) return (error = "Port invalide.");
    }
    if (sshLike && s.auth === "key" && !s.key_path) return (error = "Choisis un fichier de clé privée.");
    if (s.kind === "browser" && !s.host) return (error = "Saisis l'adresse de la page (URL).");
    if (s.kind === "serial" && !x("serial_port")) return (error = "Choisis un port série.");
    if (!s.name.trim()) s.name = defaultName();
    app.saveSession(s, isNew);
  }

  const BAUDS = ["300", "1200", "2400", "4800", "9600", "19200", "38400", "57600", "115200", "230400", "460800", "921600"];
</script>

<Modal title="Paramètres de la session" onclose={() => (app.editing = null)} width="760px">
  <div class="protos" role="radiogroup" aria-label="Type de session">
    {#each PROTOCOLS.filter((p) => p.kind !== "wsl" || IS_WINDOWS || s.kind === "wsl") as pr (pr.kind)}
      <button
        class="proto"
        class:on={s.kind === pr.kind}
        class:soon={!pr.ready}
        role="radio"
        aria-checked={s.kind === pr.kind}
        title={pr.ready ? pr.label : `${pr.label} — bientôt disponible`}
        onclick={() => pickKind(pr.kind)}
      >
        <Icon name={pr.icon} size={30} />
        <span>{pr.label}</span>
      </button>
    {/each}
  </div>

  <fieldset class="basic">
    <legend><Icon name={p.icon} size={14} /> Paramètres de base {p.label}</legend>
    {#if !p.ready}
      <p class="muted">Ce type de session sera disponible dans une prochaine étape du développement.</p>
    {:else if isRemote}
      <div class="line">
        <label for="host">Hôte distant <b class="req">*</b></label>
        <input id="host" class="host" bind:value={s.host} autocomplete="off" spellcheck="false" />
        {#if hasUser}
          <label class="chk"><input type="checkbox" bind:checked={specifyUser} /> Spécifier l'utilisateur</label>
          <input class="user" bind:value={s.username} disabled={!specifyUser} autocomplete="off" spellcheck="false" aria-label="Nom d'utilisateur" placeholder={s.kind === "ftp" ? "anonymous" : ""} />
        {/if}
        <label for="port">Port</label>
        <input id="port" class="port" type="number" min="1" max="65535" bind:value={s.port} />
      </div>
    {:else if s.kind === "serial"}
      <div class="line">
        <label for="sp">Port série <b class="req">*</b></label>
        <input id="sp" list="serial-ports" value={x("serial_port")} oninput={(e) => setX("serial_port", e.currentTarget.value)} placeholder={ports.length ? "" : "aucun port détecté (ex. COM3, /dev/ttyUSB0)"} />
        <datalist id="serial-ports">{#each ports as pt (pt)}<option value={pt}></option>{/each}</datalist>
        <button type="button" title="Rafraîchir la liste" onclick={refreshPorts}><Icon name="refresh" size={14} /></button>
        <label for="baud">Vitesse (bauds)</label>
        <select id="baud" value={x("baud", "115200")} onchange={(e) => setX("baud", e.currentTarget.value)}>
          {#each BAUDS as b (b)}<option value={b}>{b}</option>{/each}
        </select>
      </div>
    {:else if s.kind === "shell"}
      <div class="line">
        <label for="sh">Shell de la session</label>
        <select id="sh" value={x("shell", localShells[0]?.id ?? "")} onchange={(e) => setX("shell", e.currentTarget.value)}>
          {#each localShells as l (l.id)}<option value={l.id}>{l.label}</option>{/each}
        </select>
        <label for="cwd">Répertoire de démarrage</label>
        <input id="cwd" value={x("cwd")} oninput={(e) => setX("cwd", e.currentTarget.value)} placeholder="(dossier personnel)" />
        <button type="button" onclick={pickDir}>…</button>
      </div>
    {:else if s.kind === "browser"}
      <div class="line">
        <label for="host">Adresse (URL) <b class="req">*</b></label>
        <input id="host" class="host" bind:value={s.host} placeholder="https://intranet.exemple.fr" autocomplete="off" spellcheck="false" />
      </div>
    {:else if s.kind === "wsl"}
      <div class="line">
        <label for="distro">Distribution</label>
        <select id="distro" value={x("distro")} onchange={(e) => setX("distro", e.currentTarget.value)}>
          <option value="">(par défaut)</option>
          {#each wslDistros as d (d)}<option value={d}>{d}</option>{/each}
        </select>
        {#if !wslDistros.length}<span class="muted">Aucune distribution WSL détectée sur ce poste.</span>{/if}
      </div>
    {/if}
  </fieldset>

  <div class="bottom">
    <div class="btabs" role="tablist">
      <button role="tab" aria-selected={bottom === "advanced"} class:on={bottom === "advanced"} onclick={() => (bottom = "advanced")}><Icon name="settings" size={14} /> Paramètres avancés</button>
      <button role="tab" aria-selected={bottom === "terminal"} class:on={bottom === "terminal"} onclick={() => (bottom = "terminal")}><Icon name="terminal" size={14} /> Paramètres du terminal</button>
      <button role="tab" aria-selected={bottom === "network"} class:on={bottom === "network"} onclick={() => (bottom = "network")}><Icon name="network" size={14} /> Paramètres réseau</button>
      <button role="tab" aria-selected={bottom === "bookmark"} class:on={bottom === "bookmark"} onclick={() => (bottom = "bookmark")}><Icon name="star" size={14} /> Signet</button>
    </div>
    <div class="bpanel">
      {#if bottom === "advanced"}
        {#if sshLike}
          <div class="grid">
            <span class="lbl">Authentification</span>
            <div class="row">
              <label class="chk"><input type="radio" bind:group={s.auth} value="password" /> Mot de passe</label>
              <label class="chk"><input type="radio" bind:group={s.auth} value="key" /> Clé privée</label>
            </div>
            {#if s.auth === "password"}
              <span></span>
              <label class="chk"><input type="checkbox" bind:checked={s.remember_password} /> Mémoriser le mot de passe (trousseau du système)</label>
            {:else}
              <label for="key" class="lbl">Clé privée</label>
              <div class="row">
                <input id="key" bind:value={s.key_path} placeholder="~/.ssh/id_ed25519" spellcheck="false" />
                <button type="button" onclick={pickKey}>…</button>
              </div>
            {/if}
            {#if s.kind === "ssh"}
            <span class="lbl">Navigateur SFTP</span>
            <label class="chk"><input type="checkbox" bind:checked={s.sftp_enabled} /> Afficher le navigateur SFTP avec la session</label>
            <span class="lbl">Redirection X11</span>
            <div class="row">
              <label class="chk"><input type="checkbox" checked={x("x11") === "1"} onchange={(e) => setX("x11", e.currentTarget.checked ? "1" : "")} /> Activer</label>
              <input class="disp" value={x("x11_display")} oninput={(e) => setX("x11_display", e.currentTarget.value)} placeholder="affichage local (auto : 127.0.0.1:0)" disabled={x("x11") !== "1"} spellcheck="false" aria-label="Affichage X local" />
            </div>
            <span class="lbl">Compression</span>
            <label class="chk"><input type="checkbox" checked={x("compression") === "1"} onchange={(e) => setX("compression", e.currentTarget.checked ? "1" : "")} /> Compresser le trafic SSH (zlib)</label>
            <label class="lbl" for="rcmd">Commande à exécuter</label>
            <input id="rcmd" value={x("command")} oninput={(e) => setX("command", e.currentTarget.value)} placeholder="(shell interactif)" spellcheck="false" />
            {/if}
          </div>
        {:else if s.kind === "rdp"}
          {@const ext = x("rdp_client") === "mstsc"}
          <div class="grid">
            <label class="lbl" for="rcl">Client</label>
            <select id="rcl" value={x("rdp_client", "embedded")} onchange={(e) => setX("rdp_client", e.currentTarget.value === "mstsc" ? "mstsc" : "")}>
              <option value="embedded">Intégré à l'onglet (Orbiterm)</option>
              <option value="mstsc">{IS_WINDOWS ? "Fenêtre séparée : Bureau à distance de Windows (mstsc)" : "Fenêtre séparée : FreeRDP"}</option>
            </select>
            <label class="lbl" for="rsz">Taille de l'écran distant</label>
            <select id="rsz" value={x("rdp_size", "fit")} onchange={(e) => setX("rdp_size", e.currentTarget.value)}>
              <option value="fit">Adapter à l'onglet (suit le redimensionnement)</option>
              {#if ext}<option value="fullscreen">Plein écran</option>{/if}
              <option value="multimon">Tous les écrans de ce poste (multi-écran)</option>
              {#each RDP_SIZES as z (z)}<option value={z}>{z.replace("x", " × ")}</option>{/each}
            </select>
            <label class="lbl" for="rdom">Domaine</label>
            <input id="rdom" value={x("rdp_domain")} oninput={(e) => setX("rdp_domain", e.currentTarget.value)} placeholder="(aucun — ou DOMAINE\utilisateur)" spellcheck="false" />
            <label class="lbl" for="rsnd">Son</label>
            <select id="rsnd" value={x("rdp_sound", "local")} onchange={(e) => setX("rdp_sound", e.currentTarget.value)}>
              <option value="local">Lire sur cet ordinateur</option>
              <option value="remote">Lire sur l'ordinateur distant</option>
              <option value="none">Ne pas lire</option>
            </select>
            <label class="lbl" for="rgw">Passerelle RD</label>
            <input id="rgw" class="host" value={x("rdp_gateway")} oninput={(e) => setX("rdp_gateway", e.currentTarget.value)} placeholder="(aucune) — hôte[:port], HTTPS 443 par défaut" spellcheck="false" />
            {#if x("rdp_gateway")}
              <label class="lbl" for="rgwu">Compte passerelle</label>
              <input id="rgwu" value={x("rdp_gateway_user")} oninput={(e) => setX("rdp_gateway_user", e.currentTarget.value)} placeholder="(même compte que la session)" spellcheck="false" />
              {#if !ext}
                <span class="lbl">Certificat</span>
                <label class="chk" title="À n'utiliser que pour une passerelle de test au certificat auto-signé : la connexion n'est alors plus protégée contre l'interception"><input type="checkbox" checked={x("rdp_gateway_insecure") === "1"} onchange={(e) => setX("rdp_gateway_insecure", e.currentTarget.checked ? "1" : "")} /> Accepter le certificat de la passerelle même s'il n'est pas reconnu (déconseillé)</label>
              {/if}
            {/if}
            {#if !ext}
              <span class="lbl">Mot de passe</span>
              <label class="chk"><input type="checkbox" bind:checked={s.remember_password} /> Mémoriser le mot de passe (trousseau du système)</label>
              <label class="lbl" for="ros">Système distant</label>
              <select id="ros" value={x("rdp_os")} onchange={(e) => setX("rdp_os", e.currentTarget.value)}>
                <option value="">Non précisé (choisi à la fermeture de session)</option>
                <option value="windows">Windows</option>
                <option value="linux">Linux (xrdp)</option>
              </select>
              <span class="lbl">Infos système</span>
              <label class="chk" title="CPU, mémoire, disque, réseau : lus par SSH (port 22) avec le même compte, si le poste l'accepte"><input type="checkbox" checked={x("rdp_monitor") !== "0"} onchange={(e) => setX("rdp_monitor", e.currentTarget.checked ? "" : "0")} /> Afficher les informations du poste distant (via SSH, même compte)</label>
              <span class="lbl">Redirections</span>
              <div class="row">
                <label class="chk" title="Texte copié d'un côté, collé de l'autre (dans les deux sens)"><input type="checkbox" checked={x("rdp_clipboard", "1") === "1"} onchange={(e) => setX("rdp_clipboard", e.currentTarget.checked ? "1" : "0")} /> Presse-papiers partagé</label>
                <label class="chk" title="Le dossier apparaît sur le poste distant dans « Ce PC » (Windows) ou ~/thinclient_drives (xrdp)"><input type="checkbox" checked={x("rdp_drives") === "1"} onchange={(e) => setX("rdp_drives", e.currentTarget.checked ? "1" : "0")} /> Partager un dossier local</label>
              </div>
              {#if x("rdp_drives") === "1"}
                <label class="lbl" for="rshare">Dossier partagé</label>
                <div class="row">
                  <input id="rshare" value={x("rdp_share_dir")} oninput={(e) => setX("rdp_share_dir", e.currentTarget.value)} placeholder="(dossier personnel)" spellcheck="false" />
                  <button type="button" onclick={pickShare}>…</button>
                </div>
              {/if}
            {:else}
              <span class="lbl">Redirections</span>
              <div class="row">
                <label class="chk"><input type="checkbox" checked={x("rdp_clipboard", "1") === "1"} onchange={(e) => setX("rdp_clipboard", e.currentTarget.checked ? "1" : "0")} /> Presse-papiers</label>
                <label class="chk"><input type="checkbox" checked={x("rdp_drives") === "1"} onchange={(e) => setX("rdp_drives", e.currentTarget.checked ? "1" : "0")} /> Disques locaux</label>
                <label class="chk"><input type="checkbox" checked={x("rdp_printers") === "1"} onchange={(e) => setX("rdp_printers", e.currentTarget.checked ? "1" : "0")} /> Imprimantes</label>
              </div>
              <span class="lbl">Session</span>
              <label class="chk"><input type="checkbox" checked={x("rdp_admin") === "1"} onchange={(e) => setX("rdp_admin", e.currentTarget.checked ? "1" : "0")} /> Session d'administration (console)</label>
            {/if}
          </div>
          {#if ext}
            <p class="muted note">Le bureau s'ouvre dans la fenêtre du client {IS_WINDOWS ? "Bureau à distance de Windows (mstsc), qui demande et peut mémoriser le mot de passe" : "FreeRDP"}.</p>
          {:else}
            <p class="muted note">Le bureau distant s'affiche dans l'onglet (authentification NLA, certificat du serveur vérifié comme les clés SSH). Bouton « Plein écran » dans la barre de l'onglet.</p>
          {/if}
        {:else if s.kind === "vnc"}
          <div class="grid">
            <span class="lbl">Mot de passe</span>
            <label class="chk"><input type="checkbox" bind:checked={s.remember_password} /> Mémoriser le mot de passe VNC (trousseau du système)</label>
            <span class="lbl">Affichage</span>
            <div class="row">
              <label class="chk"><input type="checkbox" checked={x("vnc_scale", "1") === "1"} onchange={(e) => setX("vnc_scale", e.currentTarget.checked ? "1" : "0")} /> Ajuster à la taille de l'onglet</label>
              <label class="chk"><input type="checkbox" checked={x("vnc_viewonly") === "1"} onchange={(e) => setX("vnc_viewonly", e.currentTarget.checked ? "1" : "0")} /> Lecture seule</label>
            </div>
          </div>
        {:else if s.kind === "ftp"}
          <div class="grid">
            <label class="lbl" for="ftls">Chiffrement</label>
            <select id="ftls" value={x("ftp_tls")} onchange={(e) => setX("ftp_tls", e.currentTarget.value)}>
              <option value="">Aucun (FTP)</option>
              <option value="explicit">TLS explicite (FTPES / AUTH TLS)</option>
            </select>
            <span class="lbl">Certificat</span>
            <label class="chk"><input type="checkbox" checked={x("ftp_insecure") === "1"} disabled={x("ftp_tls") !== "explicit"} onchange={(e) => setX("ftp_insecure", e.currentTarget.checked ? "1" : "")} /> Accepter un certificat auto-signé</label>
            <span class="lbl">Mot de passe</span>
            <label class="chk"><input type="checkbox" bind:checked={s.remember_password} /> Mémoriser le mot de passe (trousseau du système)</label>
          </div>
          <p class="muted note">Sans utilisateur, la connexion est anonyme.</p>
        {:else if s.kind === "serial"}
          <div class="grid">
            <label class="lbl" for="db">Bits de données</label>
            <select id="db" value={x("data_bits", "8")} onchange={(e) => setX("data_bits", e.currentTarget.value)}>
              {#each ["5", "6", "7", "8"] as v (v)}<option value={v}>{v}</option>{/each}
            </select>
            <label class="lbl" for="par">Parité</label>
            <select id="par" value={x("parity", "none")} onchange={(e) => setX("parity", e.currentTarget.value)}>
              <option value="none">Aucune</option><option value="odd">Impaire</option><option value="even">Paire</option>
            </select>
            <label class="lbl" for="sb">Bits de stop</label>
            <select id="sb" value={x("stop_bits", "1")} onchange={(e) => setX("stop_bits", e.currentTarget.value)}>
              <option value="1">1</option><option value="2">2</option>
            </select>
            <label class="lbl" for="fl">Contrôle de flux</label>
            <select id="fl" value={x("flow", "none")} onchange={(e) => setX("flow", e.currentTarget.value)}>
              <option value="none">Aucun</option><option value="hardware">Matériel (RTS/CTS)</option><option value="software">Logiciel (XON/XOFF)</option>
            </select>
          </div>
        {:else if s.kind === "shell"}
          <div class="grid">
            <label class="lbl" for="prog">Programme personnalisé</label>
            <div class="row">
              <input id="prog" value={x("program")} oninput={(e) => setX("program", e.currentTarget.value)} placeholder="(utiliser le shell choisi ci-dessus)" spellcheck="false" />
              <button type="button" onclick={pickProgram}>…</button>
            </div>
            <label class="lbl" for="args">Arguments</label>
            <input id="args" value={x("args")} oninput={(e) => setX("args", e.currentTarget.value)} spellcheck="false" />
          </div>
        {:else if s.kind === "mosh"}
          <p class="muted">Mosh utilise le client « mosh » installé sur ce poste (il doit être présent dans le PATH).</p>
        {:else}
          <p class="muted">Aucun paramètre avancé pour ce type de session.</p>
        {/if}
      {:else if bottom === "terminal"}
        <div class="grid">
          <label class="lbl" for="ff">Police</label>
          <div class="row">
            <input id="ff" bind:value={s.terminal.font_family} spellcheck="false" />
            <input class="port" type="number" min="6" max="40" bind:value={s.terminal.font_size} aria-label="Taille de police" />
          </div>
          <label class="lbl" for="th">Thème</label>
          <select id="th" bind:value={s.terminal.theme}>
            <option value="dark">Noir</option>
            <option value="light">Clair</option>
            <option value="solarized">Solarized sombre</option>
            <option value="orbiterm">Orbiterm brun</option>
            <option value="creme">Orbiterm crème</option>
            <option value="cyber">Cyberpunk néon</option>
            <option value="graphite">Gris sombre</option>
            <option value="graphite-light">Gris clair</option>
          </select>
          <label class="lbl" for="sbk">Lignes d'historique</label>
          <input id="sbk" class="port" type="number" min="100" max="200000" step="1000" bind:value={s.terminal.scrollback} />
        </div>
      {:else if bottom === "network"}
        {#if hasGateway}
          <div class="net">
            <div class="sub"><Icon name="servers" size={14} /> Passerelle SSH (rebond / jump host)</div>
            <div class="line">
              <label for="jh">Hôte</label>
              <input id="jh" class="host" value={x("jump_host")} oninput={(e) => setX("jump_host", e.currentTarget.value)} placeholder="(aucune : connexion directe)" spellcheck="false" />
              <label for="ju">Utilisateur</label>
              <input id="ju" class="user" value={x("jump_user")} oninput={(e) => setX("jump_user", e.currentTarget.value)} placeholder={s.username || "(idem)"} spellcheck="false" />
              <label for="jp">Port</label>
              <input id="jp" class="port" type="number" min="1" max="65535" value={x("jump_port", "22")} oninput={(e) => setX("jump_port", e.currentTarget.value)} />
            </div>
            <div class="line">
              <label for="jk">Clé privée</label>
              <input id="jk" class="host" value={x("jump_key")} oninput={(e) => setX("jump_key", e.currentTarget.value)} placeholder="(sinon : mot de passe demandé)" spellcheck="false" />
              <button type="button" onclick={pickJumpKey}>…</button>
            </div>

            {#if s.kind === "ssh"}
            <div class="sub"><Icon name="tunnel" size={14} /> Redirections de ports ouvertes avec la session</div>
            <table class="fw">
              <thead><tr><th>Type</th><th>Écoute (hôte : port)</th><th>Destination (hôte : port)</th><th></th></tr></thead>
              <tbody>
                {#each s.forwards as f, i (i)}
                  <tr>
                    <td>
                      <select bind:value={f.kind} aria-label="Type">
                        <option value="local">Local (-L)</option>
                        <option value="remote">Distant (-R)</option>
                        <option value="dynamic">SOCKS (-D)</option>
                      </select>
                    </td>
                    <td><div class="pair">
                      <input bind:value={f.bind_host} placeholder={f.kind === "remote" ? "serveur" : "127.0.0.1"} aria-label="Hôte d'écoute" />
                      <input class="port" type="number" min="0" max="65535" bind:value={f.bind_port} aria-label="Port d'écoute" />
                    </div></td>
                    <td><div class="pair">
                      {#if f.kind === "dynamic"}
                        <span class="muted">(selon le client SOCKS)</span>
                      {:else}
                        <input bind:value={f.dest_host} placeholder={f.kind === "remote" ? "127.0.0.1" : "hôte vu du serveur"} aria-label="Hôte de destination" />
                        <input class="port" type="number" min="1" max="65535" bind:value={f.dest_port} aria-label="Port de destination" />
                      {/if}
                    </div></td>
                    <td><button type="button" title="Supprimer" aria-label="Supprimer la redirection" onclick={() => s.forwards.splice(i, 1)}><Icon name="trash" size={14} /></button></td>
                  </tr>
                {/each}
              </tbody>
            </table>
            <button type="button" onclick={() => s.forwards.push(newForward())}><Icon name="plus" size={12} /> Ajouter une redirection</button>
            {:else}
            <p class="muted">{s.kind === "sftp" ? "La connexion SFTP" : `La connexion ${p.label}`} passe par un tunnel SSH ouvert sur la passerelle.</p>
            {/if}
          </div>
        {:else}
          <p class="muted">Aucun paramètre réseau pour ce type de session.</p>
        {/if}
      {:else}
        <div class="grid">
          <label class="lbl" for="nm">Nom de la session</label>
          <input id="nm" bind:value={s.name} placeholder={defaultName() || "(automatique)"} />
          <label class="lbl" for="fd">Dossier</label>
          <select id="fd" bind:value={s.folder}>
            <option value="">Sessions utilisateur (racine)</option>
            {#each app.store.folders as f (f.id)}<option value={f.id}>{folderPath(f.id)}</option>{/each}
          </select>
          {#if viewOf(s) === "terminal"}
            <label class="lbl" for="smac">Macro au démarrage</label>
            <select id="smac" value={x("startup_macro")} onchange={(e) => setX("startup_macro", e.currentTarget.value)}>
              <option value="">(aucune)</option>
              {#each app.store.macros as m (m.id)}<option value={m.id}>{m.name}</option>{/each}
            </select>
          {/if}
          <label class="lbl" for="scom">Commentaire</label>
          <textarea id="scom" rows="3" value={x("comment")} oninput={(e) => setX("comment", e.currentTarget.value)} placeholder="(affiché au survol de la session)"></textarea>
        </div>
        {#if viewOf(s) === "terminal" && !app.store.macros.length}
          <p class="muted note">Enregistre d’abord une macro (menu Macros) pour la lancer automatiquement à l’ouverture de la session.</p>
        {/if}
      {/if}
    </div>
  </div>
  {#if error}<p class="error">{error}</p>{/if}

  {#snippet actions()}
    <button class="primary dlg" onclick={ok}>OK</button>
    <button class="dlg" onclick={() => (app.editing = null)}>Annuler</button>
  {/snippet}
</Modal>

<style>
  .protos {
    display: flex;
    flex-wrap: wrap;
    gap: 2px;
    padding-bottom: 8px;
    border-bottom: 1px solid var(--chrome-border);
    margin-bottom: 10px;
  }
  .proto {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    width: 58px;
    height: 56px;
    padding: 3px 0;
    border: 1px solid transparent;
    border-radius: 3px;
    background: none;
    font-size: 11.5px;
  }
  .proto:hover {
    background: var(--hover);
    border-color: var(--hover-border);
  }
  .proto.on {
    background: var(--selected);
    border-color: var(--accent);
  }
  .proto.soon {
    opacity: 0.5;
  }
  fieldset {
    border: 1px solid var(--chrome-border);
    border-radius: 3px;
    margin: 0 0 10px;
    padding: 10px 12px 12px;
    min-height: 34px;
  }
  legend {
    padding: 0 4px;
    display: flex;
    align-items: center;
    gap: 4px;
    font-weight: 600;
  }
  .line {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .line label {
    white-space: nowrap;
  }
  .host {
    flex: 1;
    min-width: 120px;
  }
  .user {
    width: 120px;
  }
  #sp,
  #cwd {
    flex: 1;
  }
  .grid select {
    max-width: 280px;
  }
  .port {
    width: 72px;
  }
  .req {
    color: var(--danger);
  }
  .bottom {
    border: 1px solid var(--chrome-border);
    border-radius: 3px;
  }
  .btabs {
    display: flex;
    background: var(--chrome);
    border-bottom: 1px solid var(--chrome-border);
  }
  .btabs button {
    display: flex;
    align-items: center;
    gap: 5px;
    border: none;
    border-right: 1px solid var(--chrome-border);
    border-radius: 0;
    background: none;
    padding: 5px 10px;
  }
  .btabs button.on {
    background: var(--panel);
    box-shadow: inset 0 2px 0 var(--accent);
  }
  .bpanel {
    padding: 12px;
    min-height: 150px;
  }
  .grid {
    display: grid;
    grid-template-columns: 170px 1fr;
    gap: 8px 12px;
    align-items: center;
  }
  .row {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .row input:not([type="radio"]):not(.port) {
    flex: 1;
  }
  .chk {
    display: flex;
    gap: 5px;
    align-items: center;
    white-space: nowrap;
  }
  .lbl {
    color: var(--muted);
  }
  .error {
    color: var(--danger);
    margin: 10px 0 0;
  }
  .dlg {
    min-width: 90px;
  }
  .note {
    margin: 10px 0 0;
  }
  .disp {
    flex: 1;
  }
  .net {
    display: flex;
    flex-direction: column;
    gap: 8px;
    align-items: flex-start;
  }
  .net .line {
    width: 100%;
  }
  .sub {
    display: flex;
    align-items: center;
    gap: 5px;
    font-weight: 600;
    margin-top: 2px;
  }
  .fw {
    border-collapse: collapse;
    width: 100%;
  }
  .fw th {
    text-align: left;
    font-weight: normal;
    color: var(--muted);
    padding: 2px 4px;
    border-bottom: 1px solid var(--chrome-border);
  }
  .fw td {
    padding: 2px 4px;
  }
  .pair {
    display: flex;
    gap: 4px;
  }
  .pair input:not(.port) {
    flex: 1;
    min-width: 80px;
  }
  .fw button {
    padding: 2px 4px;
  }
</style>
