<script lang="ts">
  import { onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import Modal from "./Modal.svelte";
  import Icon from "./Icon.svelte";
  import { app } from "./app.svelte";
  import { api, PROTOCOLS, protocol, type Session } from "./api";

  let { session }: { session: Session } = $props();

  // edit a copy, so "Annuler" really cancels
  // svelte-ignore state_referenced_locally
  let s: Session = $state(structuredClone($state.snapshot(session)) as Session);
  // svelte-ignore state_referenced_locally
  const isNew = !session.id;
  let error = $state("");
  let bottom: "advanced" | "terminal" | "network" | "bookmark" = $state("advanced");
  let ports: string[] = $state([]);
  // svelte-ignore state_referenced_locally
  let specifyUser = $state(!!session.username || session.kind === "ssh");

  const p = $derived(protocol(s.kind));
  const isRemote = $derived(["ssh", "telnet", "rlogin", "mosh"].includes(s.kind));
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
  async function pickDir() {
    const d = await open({ directory: true, title: "Répertoire de démarrage" });
    if (typeof d === "string") setX("cwd", d);
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
    if (s.kind === "ssh" && s.auth === "key" && !s.key_path) return (error = "Choisis un fichier de clé privée.");
    if (s.kind === "serial" && !x("serial_port")) return (error = "Choisis un port série.");
    if (!s.name.trim()) s.name = defaultName();
    app.saveSession(s, isNew);
  }

  const BAUDS = ["300", "1200", "2400", "4800", "9600", "19200", "38400", "57600", "115200", "230400", "460800", "921600"];
</script>

<Modal title="Paramètres de la session" onclose={() => (app.editing = null)} width="760px">
  <div class="protos" role="radiogroup" aria-label="Type de session">
    {#each PROTOCOLS as pr (pr.kind)}
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
        <label class="chk"><input type="checkbox" bind:checked={specifyUser} /> Spécifier l'utilisateur</label>
        <input class="user" bind:value={s.username} disabled={!specifyUser} autocomplete="off" spellcheck="false" aria-label="Nom d'utilisateur" />
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
        {#if s.kind === "ssh"}
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
            <span class="lbl">Navigateur SFTP</span>
            <label class="chk"><input type="checkbox" bind:checked={s.sftp_enabled} /> Afficher le navigateur SFTP avec la session</label>
            <span class="lbl">Redirection X11</span>
            <label class="chk soon-l"><input type="checkbox" disabled /> Activer (prochaine étape)</label>
          </div>
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
            <option value="dark">Noir (par défaut)</option>
            <option value="light">Clair</option>
            <option value="solarized">Solarized sombre</option>
          </select>
          <label class="lbl" for="sbk">Lignes d'historique</label>
          <input id="sbk" class="port" type="number" min="100" max="200000" step="1000" bind:value={s.terminal.scrollback} />
        </div>
      {:else if bottom === "network"}
        <p class="muted">Rebond SSH (jump host), proxy et tunnels arrivent dans une prochaine étape.</p>
      {:else}
        <div class="grid">
          <label class="lbl" for="nm">Nom de la session</label>
          <input id="nm" bind:value={s.name} placeholder={defaultName() || "(automatique)"} />
          <label class="lbl" for="fd">Dossier</label>
          <select id="fd" bind:value={s.folder}>
            <option value="">Sessions utilisateur (racine)</option>
            {#each app.store.folders as f (f.id)}<option value={f.id}>{folderPath(f.id)}</option>{/each}
          </select>
        </div>
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
  .soon-l {
    color: var(--text-disabled);
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
</style>
