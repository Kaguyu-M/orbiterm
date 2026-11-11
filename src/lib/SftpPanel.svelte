<script lang="ts">
  // Remote file browser for one SSH connection (SFTP channel of that connection).
  import { onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { api, asApiError, formatSize, remoteJoin, remoteParent, type Entry } from "./api";
  import Icon from "./Icon.svelte";
  import { app } from "./app.svelte";

  let {
    connId,
    visible,
    askText,
    confirm,
    wide = false,
    onseparate,
    source = "",
    follow = false,
  }: {
    connId: string;
    visible: boolean;
    /** full-tab layout with columns (SFTP / FTP sessions) */
    wide?: boolean;
    askText: (title: string, initial: string) => Promise<string | null>;
    confirm: (message: string) => Promise<boolean>;
    /** opens the same server in a separate, saved SFTP session */
    onseparate?: () => void;
    /** "user@host", shown by the editor tabs of remote files */
    source?: string;
    /** browser of an SSH terminal: can follow the folder of its shell */
    follow?: boolean;
  } = $props();

  let path = $state("");
  let pathInput = $state("");
  let entries: Entry[] = $state([]);
  let showHidden = $state(false);
  let selected: string | null = $state(null);
  let loading = $state(false);
  let error = $state("");
  let dropHover = $state(false);
  let panel: HTMLDivElement;

  type Transfer = { id: string; label: string; done: number; total: number; state: "run" | "ok" | "err"; msg?: string };
  let transfers: Transfer[] = $state([]);

  const selectedEntry = $derived(entries.find((e) => e.path === selected) ?? null);
  const shown = $derived(entries.filter((e) => showHidden || !e.name.startsWith(".")));

  async function load(p: string) {
    loading = true;
    error = "";
    try {
      entries = await api.sftpList(connId, p);
      path = p;
      pathInput = p;
      selected = null;
    } catch (e) {
      error = asApiError(e).message;
    } finally {
      loading = false;
    }
  }

  let scpMode = $state(false);
  async function init() {
    try {
      await load(await api.sftpHome(connId));
      scpMode = (await api.sftpMode(connId).catch(() => "sftp")) === "scp";
    } catch (e) {
      error = asApiError(e).message;
    }
  }

  let initialized = false;
  $effect(() => {
    if (visible && connId && !initialized) {
      initialized = true;
      init();
    }
  });

  // Follows the current folder of the terminal's shell (polled while shown).
  let followNote = $state("");
  $effect(() => {
    if (!follow || !visible || !app.prefs.sftpFollow || !connId) return;
    let stop = false;
    let misses = 0;
    let last = "";
    const tick = async () => {
      if (stop) return;
      try {
        const cwd = await api.sshCwd(connId);
        if (cwd === null) {
          if (++misses === 2) followNote = "Ce serveur ne permet pas de suivre le dossier du terminal.";
        } else {
          misses = 0;
          followNote = "";
          // only when the shell moved: the user may browse elsewhere meanwhile
          if (cwd !== last && cwd !== path && !loading) await load(cwd);
          last = cwd;
        }
      } catch {
        /* connection closing */
      }
      if (!stop && misses < 4) setTimeout(tick, 1500);
    };
    tick();
    return () => {
      stop = true;
    };
  });

  function toggleFollow() {
    app.prefs.sftpFollow = !app.prefs.sftpFollow;
    app.savePrefs();
    followNote = "";
  }

  const EDIT_MAX = 20 * 1024 * 1024;
  function edit(e: Entry) {
    if (e.kind === "dir") return;
    if (e.size > EDIT_MAX) {
      error = `« ${e.name} » est trop volumineux pour l'éditeur (20 Mo au plus) : utilise Télécharger.`;
      return;
    }
    app.openEditor(e.path, connId, source);
  }

  function fmtDate(t: number) {
    return t ? new Date(t * 1000).toLocaleString() : "";
  }

  function newTransfer(label: string): Transfer {
    const t: Transfer = { id: crypto.randomUUID(), label, done: 0, total: 0, state: "run" };
    transfers = [t, ...transfers].slice(0, 20);
    return transfers[0];
  }

  async function download(e: Entry) {
    const dir = await open({ directory: true, title: "Télécharger dans…" });
    if (typeof dir !== "string") return;
    const t = newTransfer(`⬇ ${e.name}`);
    try {
      await api.sftpDownload(connId, e.path, dir, t.id);
      t.state = "ok";
    } catch (err) {
      t.state = "err";
      t.msg = asApiError(err).message;
    }
  }

  async function uploadFiles(files: string[]) {
    const target = path;
    for (const f of files) {
      const name = f.split(/[\\/]/).pop() ?? f;
      const t = newTransfer(`⬆ ${name}`);
      try {
        await api.sftpUpload(connId, f, target, t.id);
        t.state = "ok";
      } catch (err) {
        t.state = "err";
        t.msg = asApiError(err).message;
      }
    }
    if (target === path) load(path);
  }

  async function pickUpload() {
    const files = await open({ multiple: true, directory: false, title: "Fichiers à envoyer" });
    if (!files) return;
    await uploadFiles(Array.isArray(files) ? files : [files]);
  }

  async function mkdir() {
    const name = await askText("Nouveau dossier", "");
    if (!name) return;
    try {
      await api.sftpMkdir(connId, remoteJoin(path, name));
      load(path);
    } catch (e) {
      error = asApiError(e).message;
    }
  }

  async function rename(e: Entry) {
    const name = await askText(`Renommer « ${e.name} »`, e.name);
    if (!name || name === e.name) return;
    try {
      await api.sftpRename(connId, e.path, remoteJoin(path, name));
      load(path);
    } catch (err) {
      error = asApiError(err).message;
    }
  }

  async function remove(e: Entry) {
    const what = e.kind === "dir" ? `le dossier « ${e.name} » et tout son contenu` : `« ${e.name} »`;
    if (!(await confirm(`Supprimer définitivement ${what} ?`))) return;
    try {
      await api.sftpRemove(connId, e.path);
      load(path);
    } catch (err) {
      error = asApiError(err).message;
    }
  }

  async function activate(e: Entry) {
    if (e.kind === "dir") load(e.path);
    else if (e.kind === "link") {
      // FTP cannot tell where a link points: try it as a folder first
      try {
        await api.sftpList(connId, e.path);
        load(e.path);
      } catch {
        edit(e);
      }
    } else edit(e);
  }

  onMount(() => {
    const unsubs: Array<() => void> = [];
    listen<{ transfer_id: string; done: number; total: number }>("transfer-progress", (ev) => {
      const t = transfers.find((x) => x.id === ev.payload.transfer_id);
      if (t) {
        t.done = ev.payload.done;
        t.total = ev.payload.total;
      }
    }).then((u) => unsubs.push(u));

    // Drag & drop of local files onto the panel = upload into the current folder.
    getCurrentWebview()
      .onDragDropEvent((ev) => {
        if (!visible) return;
        const p = ev.payload;
        const r = panel?.getBoundingClientRect();
        const inside = (pos: { x: number; y: number }) => {
          const ratio = window.devicePixelRatio || 1;
          const x = pos.x / ratio;
          const y = pos.y / ratio;
          return !!r && x >= r.left && x <= r.right && y >= r.top && y <= r.bottom;
        };
        if (p.type === "over" || p.type === "enter") dropHover = inside(p.position);
        else if (p.type === "leave") dropHover = false;
        else if (p.type === "drop") {
          dropHover = false;
          if (inside(p.position) && p.paths.length) uploadFiles(p.paths);
        }
      })
      .then((u) => unsubs.push(u));
    return () => unsubs.forEach((u) => u());
  });
</script>

<div class="sftp" class:drop={dropHover} class:wide bind:this={panel}>
  <div class="toolbar">
    <button title="Dossier parent" aria-label="Dossier parent" onclick={() => load(remoteParent(path))}><Icon name="up" size={16} /></button>
    <button title="Dossier personnel" aria-label="Dossier personnel" onclick={init}><Icon name="home" size={16} /></button>
    <button title="Rafraîchir" aria-label="Rafraîchir" onclick={() => load(path)}><Icon name="refresh" size={16} /></button>
    <button title="Nouveau dossier" aria-label="Nouveau dossier" onclick={mkdir}><Icon name="newfolder" size={16} /></button>
    <button title="Envoyer des fichiers dans ce dossier" aria-label="Envoyer des fichiers" onclick={pickUpload}><Icon name="upload" size={16} /></button>
    <button title="Modifier le fichier sélectionné dans l'éditeur (double-clic)" aria-label="Modifier" disabled={!selectedEntry || selectedEntry.kind === "dir"} onclick={() => selectedEntry && edit(selectedEntry)}><Icon name="edit" size={16} /></button>
    <button title="Télécharger le fichier sélectionné" aria-label="Télécharger" disabled={!selectedEntry || selectedEntry.kind === "dir"} onclick={() => selectedEntry && download(selectedEntry)}><Icon name="download" size={16} /></button>
    <button title="Supprimer la sélection" aria-label="Supprimer" disabled={!selectedEntry} onclick={() => selectedEntry && remove(selectedEntry)}><Icon name="trash" size={16} /></button>
    {#if onseparate}
      <button title="Ouvrir dans une session SFTP séparée (enregistrée, mot de passe mémorisé)" aria-label="Session SFTP séparée" data-sftp-separate onclick={onseparate}><Icon name="sftp" size={16} /></button>
    {/if}
    {#if follow}
      <button class="follow" class:on={app.prefs.sftpFollow} title="Suivre le dossier du terminal" aria-label="Suivre le dossier du terminal" aria-pressed={app.prefs.sftpFollow} onclick={toggleFollow}><Icon name="link" size={16} /></button>
    {/if}
    <label class="hidden-toggle" title="Afficher les fichiers cachés">
      <input type="checkbox" bind:checked={showHidden} /> .*
    </label>
  </div>
  <form onsubmit={(e) => { e.preventDefault(); load(pathInput); }}>
    <input class="path" bind:value={pathInput} spellcheck="false" />
  </form>

  {#if error}<div class="error">{error}</div>{/if}
  {#if scpMode}<div class="note" data-scp>Mode SCP : ce serveur ne propose pas SFTP (commandes du shell et transferts SCP).</div>{/if}
  {#if followNote && app.prefs.sftpFollow}<div class="note">{followNote}</div>{/if}

  {#if wide}
    <div class="cols"><span></span><span>Nom</span><span class="r">Taille</span><span>Modifié</span><span>Droits</span><span></span></div>
  {/if}
  <div class="list" class:loading>
    {#each shown as e (e.path)}
      <div
        class="item"
        class:sel={selected === e.path}
        role="button"
        tabindex="0"
        onclick={() => (selected = e.path)}
        ondblclick={() => activate(e)}
        onkeydown={(k) => {
          if (k.key === "Enter") activate(e);
          if (k.key === "Delete") remove(e);
          if (k.key === "F2") rename(e);
        }}
        title="{e.perms}  {fmtDate(e.mtime)}"
      >
        <Icon name={e.kind === "dir" ? "folder" : e.kind === "link" ? "link" : "file"} size={16} />
        <span class="nm">{e.name}</span>
        <span class="sz">{e.kind === "dir" ? "" : formatSize(e.size)}</span>
        {#if wide}<span class="dt">{fmtDate(e.mtime)}</span><span class="pm">{e.perms}</span>{/if}
        <span class="acts">
          {#if e.kind !== "dir"}<button title="Modifier dans l'éditeur" onclick={(ev) => { ev.stopPropagation(); edit(e); }}>✎</button><button title="Télécharger" onclick={(ev) => { ev.stopPropagation(); download(e); }}>⬇</button>{/if}
          <button title="Renommer (F2)" onclick={(ev) => { ev.stopPropagation(); rename(e); }}>Aa</button>
          <button title="Supprimer (Suppr)" onclick={(ev) => { ev.stopPropagation(); remove(e); }}>✕</button>
        </span>
      </div>
    {:else}
      {#if !loading && !error}<p class="muted">Dossier vide</p>{/if}
    {/each}
  </div>

  {#if transfers.length}
    <div class="transfers">
      {#each transfers.slice(0, 4) as t (t.id)}
        <div class="tr" class:err={t.state === "err"} title={t.msg ?? ""}>
          <span class="tl">{t.label}</span>
          {#if t.state === "run"}
            <progress max={t.total || 1} value={t.done}></progress>
          {:else if t.state === "ok"}
            <span class="ok">✓</span>
          {:else}
            <span>✗</span>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
  <div class="hint">Glisse des fichiers ici pour les envoyer</div>
</div>

<style>
  .sftp {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    font-size: 12.5px;
    border: 2px solid transparent;
  }
  .sftp.drop {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 8%, transparent);
  }
  .note {
    padding: 4px 8px;
    color: var(--muted);
    font-size: 11.5px;
  }
  .toolbar button.follow.on {
    background: var(--selected);
    border-color: var(--hover-border);
  }
  .toolbar {
    display: flex;
    gap: 4px;
    padding: 6px;
    align-items: center;
  }
  .toolbar {
    border-bottom: 1px solid var(--border);
    gap: 1px;
    padding: 3px 4px;
  }
  .toolbar button {
    padding: 2px 3px;
    border-color: transparent;
    background: none;
  }
  .toolbar button:disabled :global(svg) {
    opacity: 0.4;
  }
  .hidden-toggle {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 3px;
    color: var(--muted);
  }
  .path {
    width: calc(100% - 8px);
    box-sizing: border-box;
    margin: 4px;
    font-family: var(--mono);
    font-size: 12px;
  }
  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }
  .list.loading {
    opacity: 0.5;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 1px 6px;
    height: 20px;
    cursor: default;
  }
  .item:hover {
    background: var(--hover);
  }
  .item.sel {
    background: var(--selected);
  }
  .nm {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sz {
    color: var(--muted);
    font-size: 11px;
  }
  .acts {
    display: none;
  }
  .item:hover .acts {
    display: flex;
  }
  .acts button {
    padding: 0 4px;
    border: none;
    background: none;
    color: var(--muted);
    font-size: 11px;
  }
  .acts button:hover {
    color: var(--fg);
  }
  .wide {
    position: absolute;
    inset: 0;
    background: var(--panel);
    font-size: 13px;
  }
  .cols,
  .wide .item {
    display: grid;
    grid-template-columns: 18px minmax(160px, 1fr) 90px 170px 90px 70px;
    gap: 8px;
  }
  .cols {
    padding: 3px 8px;
    color: var(--muted);
    background: var(--chrome);
    border-bottom: 1px solid var(--chrome-border);
  }
  .wide .item {
    height: 22px;
    padding: 1px 8px;
    align-items: center;
  }
  .wide .sz,
  .r {
    text-align: right;
  }
  .dt,
  .pm {
    color: var(--muted);
    font-size: 12px;
  }
  .pm {
    font-family: var(--mono);
  }
  .wide .acts {
    display: flex;
    visibility: hidden;
  }
  .wide .item:hover .acts {
    visibility: visible;
  }
  .error {
    color: var(--danger);
    padding: 4px 8px;
  }
  .muted {
    color: var(--muted);
    padding: 8px;
  }
  .transfers {
    border-top: 1px solid var(--border);
    padding: 4px 8px;
  }
  .tr {
    display: flex;
    gap: 6px;
    align-items: center;
    height: 20px;
  }
  .tr.err {
    color: var(--danger);
  }
  .tl {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tr progress {
    width: 80px;
  }
  .ok {
    color: var(--ok);
  }
  .hint {
    color: var(--muted);
    font-size: 11px;
    padding: 4px 8px 6px;
    text-align: center;
  }
</style>
