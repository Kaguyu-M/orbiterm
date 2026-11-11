<script lang="ts">
  // Explorer of this computer, shown next to the remote files of an SFTP / FTP tab.
  import { api, asApiError, formatSize, type LocalEntry, type LocalPlace } from "./api";
  import Icon from "./Icon.svelte";
  import { app } from "./app.svelte";
  import { clipWrite } from "./clipboard";
  import type { MenuItem } from "./MenuBar.svelte";

  let {
    visible,
    onsend,
    onitemdrag,
    onclose,
    dropActive = false,
  }: {
    visible: boolean;
    /** sends local files / folders to the remote folder shown */
    onsend: (paths: string[]) => void;
    /** start of a drag towards the remote pane */
    onitemdrag?: (ev: PointerEvent, e: LocalEntry) => void;
    onclose?: () => void;
    /** a remote file is being dragged over this pane */
    dropActive?: boolean;
  } = $props();

  let path = $state("");
  let parent: string | null = $state(null);
  let pathInput = $state("");
  let entries: LocalEntry[] = $state([]);
  let places: LocalPlace[] = $state([]);
  let selected: string | null = $state(null);
  let showHidden = $state(false);
  let loading = $state(false);
  let error = $state("");

  const shown = $derived(entries.filter((e) => showHidden || !e.hidden));
  const selectedEntry = $derived(entries.find((e) => e.path === selected) ?? null);

  /** Folder shown (target of the downloads). */
  export function dir(): string | null {
    return path || null;
  }
  export function refresh() {
    if (path) load(path, true);
  }

  async function load(p: string, keepSelection = false) {
    loading = true;
    error = "";
    try {
      const l = await api.localList(p);
      entries = l.entries;
      path = l.path;
      parent = l.parent;
      pathInput = l.path;
      if (!keepSelection || !entries.some((e) => e.path === selected)) selected = null;
      if (app.prefs.localDir !== l.path) {
        app.prefs.localDir = l.path;
        app.savePrefs();
      }
    } catch (e) {
      error = asApiError(e).message;
    } finally {
      loading = false;
    }
  }

  let initialized = false;
  $effect(() => {
    if (visible && !initialized) {
      initialized = true;
      api.localPlaces().then((p) => (places = p), () => {});
      // last folder used, or the home folder when it is gone
      load(app.prefs.localDir || "").then(() => {
        if (!path) load("");
      });
    }
  });

  const fmtDate = (t: number) => (t ? new Date(t * 1000).toLocaleString() : "");

  function activate(e: LocalEntry) {
    if (e.kind === "dir") load(e.path);
    else api.localOpen(e.path).catch((err) => (error = asApiError(err).message));
  }

  async function mkdir() {
    const name = await app.askText("Nouveau dossier (poste local)", "");
    if (!name) return;
    try {
      selected = await api.localMkdir(path, name);
      await load(path, true);
    } catch (e) {
      error = asApiError(e).message;
    }
  }

  async function rename(e: LocalEntry) {
    const name = await app.askText("Renommer (poste local)", e.name);
    if (!name || name === e.name) return;
    try {
      selected = await api.localRename(e.path, name);
      await load(path, true);
    } catch (err) {
      error = asApiError(err).message;
    }
  }

  async function remove(e: LocalEntry) {
    if (!(await app.confirm(`Mettre « ${e.name} » à la corbeille de ce poste ?`))) return;
    try {
      await api.localTrash(e.path);
      await load(path);
    } catch (err) {
      error = asApiError(err).message;
    }
  }

  function menu(ev: MouseEvent, e: LocalEntry | null) {
    ev.preventDefault();
    ev.stopPropagation();
    if (e) selected = e.path;
    const items: MenuItem[] = [];
    if (e) {
      items.push(
        { label: e.kind === "dir" ? "Ouvrir" : "Ouvrir avec le programme par défaut", icon: e.kind === "dir" ? "folder" : "file", action: () => activate(e) },
        { label: e.kind === "dir" ? "Envoyer le dossier vers le serveur" : "Envoyer vers le serveur", icon: "upload", action: () => onsend([e.path]) },
        { sep: true },
        { label: "Renommer", icon: "edit", shortcut: "F2", action: () => rename(e) },
        { label: "Mettre à la corbeille", icon: "trash", shortcut: "Suppr", action: () => remove(e) },
        { label: "Copier le chemin", icon: "copy", action: () => clipWrite(e.path) },
        { label: "Afficher dans l'explorateur du système", icon: "folder_open", action: () => api.localReveal(e.path).catch(() => {}) },
        { sep: true },
      );
    }
    items.push(
      { label: "Nouveau dossier…", icon: "newfolder", action: mkdir },
      { label: "Ouvrir ce dossier dans l'explorateur du système", icon: "folder_open", action: () => api.localOpen(path).catch(() => {}) },
      { label: "Copier le chemin du dossier", icon: "copy", action: () => clipWrite(path) },
      { label: "Rafraîchir", icon: "refresh", action: () => load(path, true) },
    );
    app.ctx = { x: ev.clientX, y: ev.clientY, items };
  }
</script>

<div class="local" class:drop={dropActive} data-drop="local" data-local-panel>
  <div class="head">
    <Icon name="shell" size={14} /><span class="title">Poste local</span>
    {#if onclose}<button class="x" title="Masquer le volet du poste local" aria-label="Masquer le volet local" onclick={onclose}>✕</button>{/if}
  </div>
  <div class="toolbar">
    <button title="Dossier parent" aria-label="Dossier parent" disabled={!parent} onclick={() => parent && load(parent)}><Icon name="up" size={16} /></button>
    <button title="Dossier personnel" aria-label="Dossier personnel" onclick={() => load("")}><Icon name="home" size={16} /></button>
    <button title="Rafraîchir" aria-label="Rafraîchir" onclick={() => load(path, true)}><Icon name="refresh" size={16} /></button>
    <button title="Nouveau dossier" aria-label="Nouveau dossier" onclick={mkdir}><Icon name="newfolder" size={16} /></button>
    <button title="Mettre la sélection à la corbeille" aria-label="Corbeille" disabled={!selectedEntry} onclick={() => selectedEntry && remove(selectedEntry)}><Icon name="trash" size={16} /></button>
    <button class="send" title="Envoyer la sélection vers le dossier distant affiché" aria-label="Envoyer vers le serveur" disabled={!selectedEntry} onclick={() => selectedEntry && onsend([selectedEntry.path])} data-local-send
      ><Icon name="upload" size={16} /> Envoyer</button
    >
    <select class="places" aria-label="Emplacements" title="Emplacements de ce poste" value="" onchange={(e) => { const v = e.currentTarget.value; e.currentTarget.value = ""; if (v) load(v); }} data-local-places>
      <option value="" disabled selected hidden>Emplacements…</option>
      {#each places as p (p.path)}<option value={p.path}>{p.label}</option>{/each}
    </select>
    <label class="hidden-toggle" title="Afficher les fichiers cachés">
      <input type="checkbox" bind:checked={showHidden} /> .*
    </label>
  </div>
  <form onsubmit={(e) => { e.preventDefault(); load(pathInput); }}>
    <input class="path" bind:value={pathInput} spellcheck="false" aria-label="Dossier local" data-local-path />
  </form>
  {#if error}<div class="error">{error}</div>{/if}
  <div class="cols"><span></span><span>Nom</span><span class="r">Taille</span><span class="dt">Modifié</span></div>
  <div class="list" class:loading role="presentation" oncontextmenu={(ev) => menu(ev, null)}>
    {#each shown as e (e.path)}
      <div
        class="item"
        class:sel={selected === e.path}
        class:dim={e.hidden}
        role="button"
        tabindex="0"
        data-local-entry={e.name}
        onclick={() => (selected = e.path)}
        ondblclick={() => activate(e)}
        oncontextmenu={(ev) => menu(ev, e)}
        onpointerdown={(ev) => onitemdrag?.(ev, e)}
        onkeydown={(k) => {
          if (k.key === "Enter") activate(e);
          if (k.key === "Delete") remove(e);
          if (k.key === "F2") rename(e);
        }}
        title={e.path}
      >
        <Icon name={e.kind === "dir" ? "folder" : e.kind === "link" ? "link" : "file"} size={16} />
        <span class="nm">{e.name}</span>
        <span class="sz">{e.kind === "dir" ? "" : formatSize(e.size)}</span>
        <span class="dt">{fmtDate(e.mtime)}</span>
      </div>
    {:else}
      {#if !loading && !error}<p class="muted">Dossier vide</p>{/if}
    {/each}
  </div>
  <div class="hint">Glisse un fichier vers le volet distant pour l'envoyer, ou depuis le volet distant pour le télécharger ici</div>
</div>

<style>
  .local {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    background: var(--panel);
    font-size: 13px;
    border: 2px solid transparent;
    container-type: inline-size;
  }
  .cols .dt {
    color: inherit;
    font-size: inherit;
  }
  @container (max-width: 400px) {
    .cols,
    .item {
      grid-template-columns: 18px minmax(100px, 1fr) 70px;
    }
    .dt {
      display: none;
    }
  }
  .local.drop {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 8%, var(--panel));
  }
  .head {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 3px 8px;
    background: var(--pane-head);
    border-bottom: 1px solid var(--chrome-border);
    font-weight: 600;
  }
  .title {
    flex: 1;
  }
  .head .x {
    border: none;
    background: none;
    padding: 0 4px;
    color: var(--muted);
  }
  .head .x:hover {
    color: var(--text);
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: 1px;
    padding: 3px 4px;
    border-bottom: 1px solid var(--border);
  }
  .toolbar button {
    padding: 2px 3px;
    border-color: transparent;
    background: none;
  }
  .toolbar button.send {
    padding: 2px 6px;
  }
  .toolbar button:disabled {
    color: var(--text-disabled);
  }
  .toolbar button:disabled :global(svg) {
    opacity: 0.4;
  }
  .places {
    margin-left: 6px;
    max-width: 150px;
    font-size: 12px;
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
  .cols,
  .item {
    display: grid;
    grid-template-columns: 18px minmax(120px, 1fr) 80px 150px;
    gap: 8px;
  }
  .cols {
    padding: 3px 8px;
    color: var(--muted);
    background: var(--chrome);
    border-bottom: 1px solid var(--chrome-border);
  }
  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
  }
  .list.loading {
    opacity: 0.5;
  }
  .item {
    height: 22px;
    padding: 1px 8px;
    align-items: center;
    cursor: default;
    user-select: none;
  }
  .item:hover {
    background: var(--hover);
  }
  .item.sel {
    background: var(--selected);
  }
  .item.dim .nm {
    opacity: 0.6;
  }
  .nm {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sz,
  .r {
    text-align: right;
  }
  .sz,
  .dt {
    color: var(--muted);
    font-size: 12px;
  }
  .dt {
    overflow: hidden;
    white-space: nowrap;
  }
  .error {
    color: var(--danger);
    padding: 4px 8px;
  }
  .muted {
    color: var(--muted);
    padding: 8px;
  }
  .hint {
    color: var(--muted);
    font-size: 11px;
    padding: 4px 8px 6px;
    text-align: center;
  }
</style>
