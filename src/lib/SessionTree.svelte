<script lang="ts">
  import Icon from "./Icon.svelte";
  import { app } from "./app.svelte";
  import { api, asApiError, describe, protocol, type Folder, type Session } from "./api";
  import Modal from "./Modal.svelte";
  import { clickSelect } from "./selection";

  let { filter = "" }: { filter?: string } = $props();

  let collapsed: Record<string, boolean> = $state({});
  let rootCollapsed = $state(false);
  /** selected sessions (`selected` is the row a Shift+click extends from) */
  let selected: string | null = $state(null);
  let multi: string[] = $state([]);
  let selectedFolder: string | null = $state(null);
  let dropTarget: string | null = $state(null);
  let tree: HTMLDivElement;

  const byName = (a: { name: string }, b: { name: string }) =>
    a.name.localeCompare(b.name, undefined, { sensitivity: "base" });

  function matches(s: Session) {
    const f = filter.trim().toLowerCase();
    return !f || `${s.name} ${s.host} ${s.username} ${s.kind}`.toLowerCase().includes(f);
  }
  function childFolders(parent: string) {
    return app.store.folders.filter((f) => f.parent === parent).sort(byName);
  }
  function childSessions(folder: string) {
    return app.store.sessions.filter((s) => s.folder === folder && matches(s)).sort(byName);
  }
  function folderHasMatch(id: string): boolean {
    if (!filter.trim()) return true;
    return childSessions(id).length > 0 || childFolders(id).some((f) => folderHasMatch(f.id));
  }

  /** Sessions in the order they are displayed (Shift+click ranges). */
  function visibleSessions(): string[] {
    const out: string[] = [];
    const walk = (folder: string) => {
      for (const f of childFolders(folder)) if (folderHasMatch(f.id) && (!collapsed[f.id] || !!filter)) walk(f.id);
      for (const s of childSessions(folder)) out.push(s.id);
    };
    if (!rootCollapsed) walk("");
    return out;
  }
  function pick(ev: MouseEvent, s: Session) {
    if (dragged) return; // the click that ends a drag
    const r = clickSelect(visibleSessions(), { ids: multi, anchor: selected }, s.id, ev);
    multi = r.ids;
    selected = r.anchor;
    selectedFolder = null;
  }
  const picked = () => app.store.sessions.filter((s) => multi.includes(s.id));
  /** sessions an action started on `s` applies to: the whole selection when `s` is part of it */
  const group = (s: Session) => (multi.length > 1 && multi.includes(s.id) ? picked() : [s]);

  function sessionMenu(e: MouseEvent, s: Session) {
    e.preventDefault();
    if (!multi.includes(s.id)) {
      multi = [s.id];
      selected = s.id;
    }
    selectedFolder = null;
    const all = group(s);
    if (all.length > 1) {
      app.ctx = {
        x: e.clientX,
        y: e.clientY,
        items: [
          { label: `Exécuter les ${all.length} sessions`, icon: "play", action: () => all.forEach((x) => app.openSession(x)) },
          { label: `Déplacer les ${all.length} sessions vers…`, icon: "folder", action: () => askMove({ sessions: all.map((x) => x.id) }) },
          { label: `Exporter les ${all.length} sessions vers un fichier…`, icon: "upload", action: () => app.exportSessions({ sessions: all.map((x) => x.id) }) },
          { sep: true },
          { label: "Nouvelle session ici…", icon: "session", action: () => app.newSession(s.folder) },
          { label: "Nouveau dossier ici…", icon: "newfolder", action: () => app.newFolder(s.folder) },
          { label: "Importer des sessions depuis un fichier…", icon: "download", action: () => app.importSessions(s.folder) },
        ],
      };
      return;
    }
    app.ctx = {
      x: e.clientX,
      y: e.clientY,
      items: [
        { label: "Exécuter", icon: "play", action: () => app.openSession(s) },
        { sep: true },
        { label: "Modifier la session", icon: "edit", action: () => (app.editing = s) },
        { label: "Dupliquer la session", icon: "copy", action: () => app.duplicate(s) },
        { label: "Renommer la session", icon: "edit", action: () => rename(s) },
        { label: "Déplacer vers…", icon: "folder", action: () => askMove({ sessions: [s.id] }) },
        { label: "Supprimer la session", icon: "trash", action: () => app.deleteSession(s) },
        { sep: true },
        {
          label: "Oublier le mot de passe enregistré",
          icon: "key",
          disabled: s.kind !== "ssh" || !s.remember_password,
          action: () => api.passwordForget(s.id).then(() => app.info("Mot de passe supprimé du trousseau.")),
        },
        { sep: true },
        { label: "Nouvelle session ici…", icon: "session", action: () => app.newSession(s.folder) },
        { label: "Nouveau dossier ici…", icon: "newfolder", action: () => app.newFolder(s.folder) },
        { sep: true },
        { label: "Exporter la session vers un fichier…", icon: "upload", action: () => app.exportSessions({ sessions: [s.id] }, s.name) },
        { label: "Importer des sessions depuis un fichier…", icon: "download", action: () => app.importSessions(s.folder) },
      ],
    };
  }

  function folderMenu(e: MouseEvent, f: Folder | null) {
    e.preventDefault();
    const id = f?.id ?? "";
    selectedFolder = f?.id ?? null;
    multi = [];
    selected = null;
    app.ctx = {
      x: e.clientX,
      y: e.clientY,
      items: [
        { label: "Nouvelle session…", icon: "session", action: () => app.newSession(id) },
        { label: "Nouveau dossier…", icon: "newfolder", action: () => app.newFolder(id) },
        ...(f
          ? [
              { sep: true as const },
              { label: "Renommer le dossier", icon: "edit", action: () => app.renameFolder(f) },
              { label: "Déplacer le dossier vers…", icon: "folder", action: () => askMove({ sessions: [], folder: f.id }) },
              { label: "Supprimer le dossier", icon: "trash", action: () => app.deleteFolder(f) },
            ]
          : []),
        { sep: true },
        {
          label: f ? "Exporter ce dossier vers un fichier…" : "Exporter toutes les sessions vers un fichier…",
          icon: "upload",
          disabled: app.store.sessions.length === 0,
          action: () => app.exportSessions({ folder: id }, f?.name ?? "sessions"),
        },
        { label: f ? "Importer des sessions dans ce dossier…" : "Importer des sessions depuis un fichier…", icon: "download", action: () => app.importSessions(id) },
        ...(f ? [] : [{ label: "Importer depuis MobaXterm, PuTTY, OpenSSH…", icon: "download", action: () => (app.showImport = true) }]),
        { sep: true },
        {
          label: "Tout déplier",
          action: () => {
            collapsed = {};
            rootCollapsed = false;
          },
        },
        {
          label: "Tout replier",
          action: () => {
            collapsed = Object.fromEntries(app.store.folders.map((x) => [x.id, true]));
          },
        },
      ],
    };
  }

  async function rename(s: Session) {
    const name = await app.askText("Renommer la session", s.name);
    if (name && name !== s.name) await app.saveSession({ ...$state.snapshot(s), name } as Session);
  }

  // ---- moving sessions / folders into a folder
  type Moved = { sessions: string[]; folder?: string };

  const folderPath = (id: string): string => {
    const f = app.store.folders.find((x) => x.id === id);
    return f ? `${folderPath(f.parent)} / ${f.name}` : "Sessions utilisateur";
  };
  /** true when `id` is `ancestor` or one of its sub-folders */
  function isInside(id: string, ancestor: string): boolean {
    for (let cur = id; cur; cur = app.store.folders.find((x) => x.id === cur)?.parent ?? "") if (cur === ancestor) return true;
    return false;
  }
  const canDrop = (m: Moved, target: string) => !m.folder || !isInside(target, m.folder);

  async function moveTo(m: Moved, target: string) {
    if (!canDrop(m, target)) return;
    try {
      let changed = false;
      for (const id of m.sessions) {
        const s = app.store.sessions.find((x) => x.id === id);
        if (s && s.folder !== target) {
          await api.sessionSave({ ...$state.snapshot(s), folder: target } as Session);
          changed = true;
        }
      }
      if (m.folder) {
        const f = app.store.folders.find((x) => x.id === m.folder);
        if (f && f.parent !== target) {
          await api.folderSave({ ...$state.snapshot(f), parent: target });
          changed = true;
        }
      }
      if (changed) {
        await app.reload();
        if (target) collapsed[target] = false; // show where the items went
      }
    } catch (err) {
      app.alert(asApiError(err).message);
      await app.reload();
    }
  }

  // "Déplacer vers…": the destination is chosen in a list (keyboard friendly)
  let moving: Moved | null = $state(null);
  let moveTarget = $state("");
  const destinations = $derived(
    moving
      ? [{ id: "", label: "Sessions utilisateur" }, ...app.store.folders.map((f) => ({ id: f.id, label: folderPath(f.id) })).sort((a, b) => a.label.localeCompare(b.label, undefined, { sensitivity: "base" }))].filter((d) =>
          canDrop(moving!, d.id),
        )
      : [],
  );
  function askMove(m: Moved) {
    moving = m;
    moveTarget = "";
  }
  async function confirmMove() {
    const m = moving;
    moving = null;
    if (m) await moveTo(m, moveTarget);
  }

  // Drag with the mouse. The native drop handler of the window (files dragged from the
  // system) disables HTML5 drag & drop in the page: this one is made of pointer events.
  type Drag = { what: Moved; label: string; x: number; y: number };
  let drag: Drag | null = $state(null);
  let dragged = false;

  /** Folder under the pointer: a folder row, the folder of a session row, the root below the rows. */
  function folderAt(x: number, y: number): string | null {
    const el = document.elementFromPoint(x, y);
    if (!el || !tree.contains(el)) return null;
    return el.closest<HTMLElement>("[data-drop-folder]")?.dataset.dropFolder ?? "";
  }

  function dragStart(ev: PointerEvent, what: Moved, label: string) {
    if (ev.button !== 0 || (ev.target as HTMLElement).closest("button")) return;
    const sx = ev.clientX;
    const sy = ev.clientY;
    dragged = false;
    const move = (m: PointerEvent) => {
      if (!drag && Math.hypot(m.clientX - sx, m.clientY - sy) < 6) return;
      dragged = true;
      drag = { what, label, x: m.clientX, y: m.clientY };
      const t = folderAt(m.clientX, m.clientY);
      dropTarget = t !== null && canDrop(what, t) ? t : null;
    };
    const end = (u: PointerEvent) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", end);
      window.removeEventListener("pointercancel", end);
      const d = drag;
      const target = dropTarget;
      drag = null;
      dropTarget = null;
      // the click that follows the release must not change the selection
      setTimeout(() => (dragged = false), 0);
      if (d && u.type === "pointerup" && target !== null) moveTo(d.what, target);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", end);
    window.addEventListener("pointercancel", end);
  }

  function dragSession(ev: PointerEvent, s: Session) {
    const all = group(s);
    dragStart(ev, { sessions: all.map((x) => x.id) }, all.length > 1 ? `${all.length} sessions` : s.name);
  }
</script>

{#snippet sessionRow(s: Session, depth: number)}
  <div
    class="row"
    class:sel={multi.includes(s.id)}
    style:padding-left="{6 + depth * 16}px"
    role="treeitem"
    aria-selected={multi.includes(s.id)}
    tabindex="0"
    data-drop-folder={s.folder}
    title={s.extra?.comment?.trim() ? `${describe(s)}\n\n${s.extra.comment.trim()}` : describe(s)}
    data-session={s.name}
    onpointerdown={(e) => dragSession(e, s)}
    onclick={(e) => pick(e, s)}
    ondblclick={() => app.openSession(s)}
    oncontextmenu={(e) => sessionMenu(e, s)}
    onkeydown={(e) => {
      if (e.key === "Enter") app.openSession(s);
      if (e.key === "Delete") app.deleteSession(s);
      if (e.key === "F2") rename(s);
    }}
  >
    <span class="tw"></span>
    <Icon name={protocol(s.kind).icon} size={16} />
    <span class="name">{s.name}</span>
  </div>
{/snippet}

{#snippet folderNode(f: Folder, depth: number)}
  {#if folderHasMatch(f.id)}
    {@const isOpen = !collapsed[f.id] || !!filter}
    <div
      class="row folder"
      class:drop={dropTarget === f.id}
      class:sel={selectedFolder === f.id}
      style:padding-left="{6 + depth * 16}px"
      role="treeitem"
      aria-selected={selectedFolder === f.id}
      aria-expanded={isOpen}
      tabindex="0"
      data-folder={f.name}
      data-drop-folder={f.id}
      onpointerdown={(e) => dragStart(e, { sessions: [], folder: f.id }, f.name)}
      onclick={() => {
        if (dragged) return;
        selectedFolder = f.id;
        multi = [];
        selected = null;
      }}
      ondblclick={() => (collapsed[f.id] = !collapsed[f.id])}
      oncontextmenu={(e) => folderMenu(e, f)}
      onkeydown={(e) => e.key === "Enter" && (collapsed[f.id] = !collapsed[f.id])}
    >
      <button class="tw" aria-label="Déplier" onclick={() => (collapsed[f.id] = !collapsed[f.id])}>{isOpen ? "▾" : "▸"}</button>
      <Icon name={isOpen ? "folder_open" : "folder"} size={16} />
      <span class="name">{f.name}</span>
    </div>
    {#if isOpen}
      {#each childFolders(f.id) as sub (sub.id)}
        {@render folderNode(sub, depth + 1)}
      {/each}
      {#each childSessions(f.id) as s (s.id)}
        {@render sessionRow(s, depth + 1)}
      {/each}
    {/if}
  {/if}
{/snippet}

<!-- a right click under the rows (empty space) gives the menu of the root -->
<div class="tree" class:dragging={!!drag} role="tree" tabindex="-1" aria-multiselectable="true" bind:this={tree} oncontextmenu={(e) => !e.defaultPrevented && folderMenu(e, null)} data-session-tree>
  <div
    class="row folder root"
    class:drop={dropTarget === ""}
    role="treeitem"
    aria-selected="false"
    aria-expanded={!rootCollapsed}
    tabindex="0"
    data-drop-folder=""
    ondblclick={() => (rootCollapsed = !rootCollapsed)}
    oncontextmenu={(e) => folderMenu(e, null)}
    onkeydown={(e) => e.key === "Enter" && (rootCollapsed = !rootCollapsed)}
  >
    <button class="tw" aria-label="Déplier" onclick={() => (rootCollapsed = !rootCollapsed)}>{rootCollapsed ? "▸" : "▾"}</button>
    <Icon name="star" size={16} />
    <span class="name">Sessions utilisateur</span>
  </div>
  {#if !rootCollapsed}
    {#each childFolders("") as f (f.id)}
      {@render folderNode(f, 1)}
    {/each}
    {#each childSessions("") as s (s.id)}
      {@render sessionRow(s, 1)}
    {/each}
  {/if}
  {#if app.store.sessions.length === 0 && app.store.folders.length === 0}
    <p class="empty">Aucune session enregistrée.<br />Bouton <b>Session</b> de la barre d'outils, ou clic droit ici.</p>
  {/if}
  {#if drag}
    <div class="ghost" class:ok={dropTarget !== null} style:left="{drag.x + 12}px" style:top="{drag.y + 14}px">{drag.label}{dropTarget !== null ? ` → ${folderPath(dropTarget)}` : ""}</div>
  {/if}
</div>

{#if moving}
  <Modal title={moving.folder ? "Déplacer le dossier" : moving.sessions.length > 1 ? `Déplacer ${moving.sessions.length} sessions` : "Déplacer la session"} onclose={() => (moving = null)} width="420px">
    <label for="mv-dest">Dossier de destination</label>
    <select id="mv-dest" class="dest" size={Math.min(Math.max(destinations.length, 3), 12)} bind:value={moveTarget} ondblclick={confirmMove} data-move-dest>
      {#each destinations as d (d.id)}<option value={d.id}>{d.label}</option>{/each}
    </select>
    {#snippet actions()}
      <button class="primary dlg" onclick={confirmMove} data-move-ok>Déplacer</button>
      <button class="dlg" onclick={() => (moving = null)}>Annuler</button>
    {/snippet}
  </Modal>
{/if}

<style>
  .tree {
    user-select: none;
    padding: 2px 0;
    /* the empty space under the rows is a drop zone too (root folder) */
    min-height: 100%;
    box-sizing: border-box;
  }
  .tree.dragging,
  .tree.dragging .row {
    cursor: grabbing;
  }
  .ghost {
    position: fixed;
    z-index: 50;
    pointer-events: none;
    padding: 2px 8px;
    border-radius: 4px;
    border: 1px solid var(--chrome-border-dark);
    background: var(--panel);
    color: var(--muted);
    box-shadow: 0 2px 8px var(--shadow);
    font-size: 12px;
    white-space: nowrap;
  }
  .ghost.ok {
    color: var(--text);
    border-color: var(--accent);
  }
  .dest {
    width: 100%;
    margin-top: 4px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 5px;
    height: 20px;
    padding-right: 4px;
    cursor: default;
    white-space: nowrap;
    border: 1px solid transparent;
  }
  .row:hover {
    background: var(--hover);
  }
  .row.sel,
  .row:focus-visible {
    background: var(--selected);
    border-color: var(--hover-border);
    outline: none;
  }
  .row.drop {
    background: var(--selected);
    border: 1px dashed var(--accent);
  }
  .root .name {
    font-weight: 600;
  }
  .name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tw {
    width: 12px;
    flex: 0 0 12px;
    padding: 0;
    border: none;
    background: none;
    color: var(--muted);
    font-size: 10px;
    line-height: 1;
  }
  .empty {
    color: var(--muted);
    padding: 12px 10px;
    line-height: 1.6;
    white-space: normal;
  }
</style>
