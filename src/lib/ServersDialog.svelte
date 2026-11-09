<script lang="ts">
  import { onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import Modal from "./Modal.svelte";
  import Icon from "./Icon.svelte";
  import { app } from "./app.svelte";
  import { api, asApiError, type ServerConfig, type ServerKind, type ServerStatus } from "./api";

  const META: Record<ServerKind, { name: string; icon: string; desc: string; scheme: string }> = {
    http: { name: "Serveur HTTP", icon: "web", desc: "Partage un dossier par le web (navigateur, wget, curl)", scheme: "http" },
    tftp: { name: "Serveur TFTP", icon: "network", desc: "Transferts simples : équipements réseau, démarrage PXE", scheme: "tftp" },
    ftp: { name: "Serveur FTP", icon: "ftp", desc: "Partage de fichiers FTP, avec compte ou anonyme", scheme: "ftp" },
    ssh: { name: "Serveur SSH/SFTP", icon: "ssh", desc: "Shell de ce poste à distance, et fichiers par SFTP", scheme: "ssh" },
  };
  const ORDER: ServerKind[] = ["http", "tftp", "ftp", "ssh"];

  let edit: ServerConfig | null = $state(null);
  let password = $state("");
  let dirty = $state(false);
  let saving = $state(false);
  let error = $state("");
  let busy: ServerKind | null = $state(null);

  const sel = $derived(app.servers.find((s) => s.config.kind === app.serverFocus));
  const meta = $derived(META[app.serverFocus]);

  function pick(kind: ServerKind) {
    app.serverFocus = kind;
    load();
  }
  function load() {
    const s = app.servers.find((x) => x.config.kind === app.serverFocus);
    edit = s ? structuredClone($state.snapshot(s.config)) as ServerConfig : null;
    password = "";
    dirty = false;
    error = "";
  }

  onMount(() => {
    app.refreshServers().then(load);
    const t = setInterval(() => app.refreshServers(), 1000);
    return () => clearInterval(t);
  });

  async function toggle(kind: ServerKind, ev?: Event) {
    ev?.stopPropagation();
    if (dirty && kind === app.serverFocus) await save();
    if (error) return;
    busy = kind;
    await app.toggleServer(kind);
    busy = null;
  }

  async function save() {
    if (!edit) return;
    error = "";
    if (!edit.port || edit.port < 1 || edit.port > 65535) return (error = "Port invalide (1 à 65535).");
    if ((edit.kind === "ftp" || edit.kind === "ssh") && !edit.user.trim()) return (error = "Indique le nom d'utilisateur.");
    saving = true;
    try {
      await api.serverSave($state.snapshot(edit) as ServerConfig, password || undefined);
      password = "";
      dirty = false;
      await app.refreshServers();
    } catch (e) {
      error = asApiError(e).message;
    } finally {
      saving = false;
    }
  }

  async function browse() {
    if (!edit) return;
    const d = await open({ directory: true, title: "Dossier partagé", defaultPath: sel?.root });
    if (typeof d === "string") {
      edit.root = d;
      dirty = true;
    }
  }

  function urls(s: ServerStatus): string[] {
    const m = META[s.config.kind];
    const user = s.config.kind === "ssh" || (s.config.kind === "ftp" && !s.config.anonymous) ? `${s.config.user}@` : "";
    const def: Record<ServerKind, number> = { http: 80, tftp: 69, ftp: 21, ssh: 22 };
    return s.addresses.map((a) => {
      const port = s.port === def[s.config.kind] ? "" : `:${s.port}`;
      return s.config.kind === "ssh" ? `ssh ${user}${a}${port ? " -p " + s.port : ""}` : `${m.scheme}://${user}${a}${port}/`;
    });
  }

  const time = (t: number) => new Date(t).toLocaleTimeString("fr-FR");
  const uptime = (s: ServerStatus) => {
    const sec = Math.max(0, Math.floor((Date.now() - s.started) / 1000));
    const h = Math.floor(sec / 3600), m = Math.floor((sec % 3600) / 60);
    return h ? `${h} h ${m} min` : m ? `${m} min ${sec % 60} s` : `${sec} s`;
  };

  let logBox: HTMLDivElement | undefined = $state();
  $effect(() => {
    void sel?.log.length;
    if (logBox) logBox.scrollTop = logBox.scrollHeight;
  });
</script>

<Modal title="Serveurs intégrés" onclose={() => (app.showServers = false)} width="940px">
  <div class="layout">
    <div class="list" role="listbox" aria-label="Serveurs">
      {#each ORDER as k (k)}
        {@const s = app.servers.find((x) => x.config.kind === k)}
        <div
          class="srv"
          class:on={app.serverFocus === k}
          role="option"
          tabindex="0"
          aria-selected={app.serverFocus === k}
          data-server={k}
          onclick={() => pick(k)}
          onkeydown={(e) => e.key === "Enter" && pick(k)}
        >
          <Icon name={META[k].icon} size={32} />
          <div class="txt">
            <div class="nm">{META[k].name}</div>
            <div class="st">
              <span class="led" class:run={s?.running}></span>
              {#if s?.running}
                port {s.port}{s.clients ? ` · ${s.clients} client(s)` : ""}
              {:else}
                arrêté
              {/if}
            </div>
          </div>
          <button
            class="sw"
            class:run={s?.running}
            disabled={busy === k}
            title={s?.running ? "Arrêter" : "Démarrer"}
            aria-label={(s?.running ? "Arrêter " : "Démarrer ") + META[k].name}
            onclick={(e) => toggle(k, e)}
          >
            <span class="knob"></span>
          </button>
        </div>
      {/each}
      <p class="hint">Les serveurs partagent un dossier de ce poste. Ils s'arrêtent à la fermeture d'Orbiterm.</p>
    </div>

    <div class="detail">
      {#if edit && sel}
        <div class="head">
          <Icon name={meta.icon} size={40} />
          <div>
            <h3>{meta.name}</h3>
            <p class="muted">{meta.desc}</p>
          </div>
          <button class="primary go" class:stop={sel.running} disabled={busy !== null} onclick={() => toggle(sel.config.kind)}>
            <Icon name={sel.running ? "stop" : "play"} size={16} />
            {sel.running ? "Arrêter" : "Démarrer"}
          </button>
        </div>

        {#if sel.running}
          <div class="access">
            <div><b>En service</b> depuis {uptime(sel)} · {sel.total} connexion(s)</div>
            {#each urls(sel) as u (u)}<code>{u}</code>{/each}
          </div>
        {/if}

        <div class="form" oninput={() => (dirty = true)} onchange={() => (dirty = true)}>
          <label for="sp">Port</label>
          <input id="sp" type="number" min="1" max="65535" bind:value={edit.port} />

          <label for="sr">Dossier partagé</label>
          <span class="row">
            <input id="sr" bind:value={edit.root} placeholder={sel.root} spellcheck="false" />
            <button title="Choisir le dossier" onclick={browse}><Icon name="folder_open" size={16} /></button>
          </span>

          <span></span>
          <label class="chk"><input type="checkbox" bind:checked={edit.local_only} /> Accessible depuis ce poste uniquement (127.0.0.1)</label>

          {#if edit.kind !== "http"}
            <span></span>
            <label class="chk"><input type="checkbox" bind:checked={edit.writable} /> Autoriser l'envoi et la modification de fichiers</label>
          {/if}

          {#if edit.kind === "http"}
            <span></span>
            <label class="chk"><input type="checkbox" bind:checked={edit.listing} /> Afficher le contenu des dossiers (sans index.html)</label>
          {/if}

          {#if edit.kind === "ftp" || edit.kind === "ssh"}
            <label for="su">Utilisateur</label>
            <input id="su" bind:value={edit.user} spellcheck="false" autocomplete="off" />
            <label for="spw">Mot de passe</label>
            <span class="row">
              <input id="spw" type="password" bind:value={password} autocomplete="new-password"
                placeholder={sel.has_password ? "•••••••• (enregistré — laisser vide pour le garder)" : "à définir"} />
            </span>
          {/if}

          {#if edit.kind === "ftp"}
            <span></span>
            <label class="chk"><input type="checkbox" bind:checked={edit.anonymous} /> Accès anonyme (utilisateur « anonymous »)</label>
          {/if}

          {#if edit.kind === "ssh"}
            <label for="ssh-shell">Shell proposé</label>
            <select id="ssh-shell" bind:value={edit.shell}>
              <option value="">Par défaut ({app.shells[0]?.label ?? "système"})</option>
              {#each app.shells as sh (sh.id)}<option value={sh.id}>{sh.label}</option>{/each}
            </select>
          {/if}

          <span></span>
          <label class="chk"><input type="checkbox" bind:checked={edit.autostart} /> Démarrer avec Orbiterm</label>
        </div>

        <div class="savebar">
          {#if error}<span class="error">{error}</span>{/if}
          <span class="grow"></span>
          {#if dirty}<span class="muted">Modifications non enregistrées{sel.running ? " (le serveur redémarrera)" : ""}</span>{/if}
          <button onclick={load} disabled={!dirty || saving}>Annuler</button>
          <button class="primary" onclick={save} disabled={!dirty || saving}>Enregistrer</button>
        </div>

        <div class="loghead">
          <span>Journal d'activité</span>
          <button class="small" onclick={() => api.serverClearLog(sel.config.kind).then(() => app.refreshServers())}>Effacer</button>
        </div>
        <div class="log" bind:this={logBox} data-log>
          {#each sel.log as l, i (i)}
            <div><span class="t">{time(l.t)}</span> {l.msg}</div>
          {:else}
            <div class="muted">Aucune activité.</div>
          {/each}
        </div>
      {/if}
    </div>
  </div>

  {#snippet actions()}
    <button class="primary dlg" onclick={() => (app.showServers = false)}>Fermer</button>
  {/snippet}
</Modal>

<style>
  .layout {
    display: grid;
    grid-template-columns: 290px 1fr;
    gap: 10px;
    min-height: 470px;
  }
  .list {
    border: 1px solid var(--chrome-border);
    background: var(--chrome);
    padding: 4px;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .srv {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 7px 8px;
    border: 1px solid transparent;
    border-radius: 3px;
    background: var(--panel);
    cursor: default;
  }
  .srv:hover {
    border-color: var(--hover-border);
  }
  .srv.on {
    background: var(--selected);
    border-color: var(--accent);
  }
  .txt {
    flex: 1;
    min-width: 0;
  }
  .nm {
    font-weight: 600;
  }
  .st {
    color: var(--muted);
    font-size: 11.5px;
    display: flex;
    align-items: center;
    gap: 5px;
  }
  .led {
    display: inline-block;
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: #c9c9c9;
    border: 1px solid #999;
  }
  .led.run {
    background: #2fbf5a;
    border-color: #1f7035;
  }
  /* on/off switch */
  .sw {
    position: relative;
    width: 40px;
    height: 20px;
    padding: 0;
    border-radius: 10px;
    border: 1px solid #9aa3ae;
    background: #d5d9df;
  }
  .sw .knob {
    position: absolute;
    top: 1px;
    left: 1px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.35);
    transition: left 0.15s;
  }
  .sw.run {
    background: #2fbf5a;
    border-color: #1f7035;
  }
  .sw.run .knob {
    left: 21px;
  }
  .hint {
    color: var(--muted);
    font-size: 11.5px;
    margin: auto 4px 2px;
  }
  .detail {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .head h3 {
    margin: 0;
    font-size: 15px;
  }
  .head p {
    margin: 2px 0 0;
  }
  .head .go {
    margin-left: auto;
    min-width: 110px;
    justify-content: center;
  }
  .go.stop {
    background: #c0392b;
    border-color: #922b21;
  }
  .access {
    background: #eefaf1;
    border: 1px solid #9fd8b0;
    padding: 6px 8px;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .access code {
    font-family: var(--mono);
    font-size: 12px;
    user-select: text;
  }
  .form {
    display: grid;
    grid-template-columns: 130px 1fr;
    align-items: center;
    gap: 6px 8px;
  }
  .row {
    display: flex;
    gap: 4px;
  }
  .row input {
    flex: 1;
  }
  .form input[type="number"] {
    width: 90px;
  }
  .chk {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .savebar {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .grow {
    flex: 1;
  }
  .error {
    color: var(--danger);
  }
  .muted {
    color: var(--muted);
  }
  .loghead {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-weight: 600;
    border-top: 1px solid var(--chrome-border);
    padding-top: 6px;
  }
  .small {
    padding: 1px 8px;
    font-size: 11.5px;
  }
  .log {
    flex: 1;
    min-height: 120px;
    max-height: 180px;
    overflow: auto;
    background: #1d2027;
    color: #d6dde6;
    font-family: var(--mono);
    font-size: 12px;
    padding: 5px 7px;
    user-select: text;
  }
  .log .t {
    color: #7fa8d6;
  }
  .log .muted {
    color: #8a94a3;
  }
  .dlg {
    min-width: 90px;
    justify-content: center;
  }
</style>
