<script lang="ts">
  import Icon from "./Icon.svelte";
  import { app } from "./app.svelte";
  import { api, describe, protocol, type Folder, type Session } from "./api";

  let { filter = "" }: { filter?: string } = $props();

  let collapsed: Record<string, boolean> = $state({});
  let rootCollapsed = $state(false);
  let selected: string | null = $state(null);
  let dropTarget: string | null = $state(null);

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

  function sessionMenu(e: MouseEvent, s: Session) {
    e.preventDefault();
    selected = s.id;
    app.ctx = {
      x: e.clientX,
      y: e.clientY,
      items: [
        { label: "Exécuter", icon: "play", action: () => app.openSession(s) },
        { sep: true },
        { label: "Modifier la session", icon: "edit", action: () => (app.editing = s) },
        { label: "Dupliquer la session", icon: "copy", action: () => app.duplicate(s) },
        { label: "Renommer la session", icon: "edit", action: () => rename(s) },
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
      ],
    };
  }

  function folderMenu(e: MouseEvent, f: Folder | null) {
    e.preventDefault();
    const id = f?.id ?? "";
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
              { label: "Supprimer le dossier", icon: "trash", action: () => app.deleteFolder(f) },
            ]
          : []),
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

  // ---- drag & drop of sessions / folders between folders
  function dragStart(e: DragEvent, kind: "s" | "f", id: string) {
    e.dataTransfer?.setData("text/orbiterm", `${kind}:${id}`);
    if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
  }
  function dragOver(e: DragEvent, folderId: string) {
    if (!e.dataTransfer?.types.includes("text/orbiterm")) return;
    e.preventDefault();
    dropTarget = folderId;
  }
  async function drop(e: DragEvent, folderId: string) {
    e.preventDefault();
    dropTarget = null;
    const v = e.dataTransfer?.getData("text/orbiterm") ?? "";
    const [kind, id] = v.split(":");
    try {
      if (kind === "s") {
        const s = app.store.sessions.find((x) => x.id === id);
        if (s && s.folder !== folderId) await app.saveSession({ ...$state.snapshot(s), folder: folderId } as Session);
      } else if (kind === "f" && id !== folderId) {
        const f = app.store.folders.find((x) => x.id === id);
        if (f && f.parent !== folderId) {
          await api.folderSave({ ...f, parent: folderId });
          await app.reload();
        }
      }
    } catch (err) {
      app.alert(String((err as { message?: string }).message ?? err));
    }
  }
</script>

{#snippet sessionRow(s: Session, depth: number)}
  <div
    class="row"
    class:sel={selected === s.id}
    style:padding-left="{6 + depth * 16}px"
    role="treeitem"
    aria-selected={selected === s.id}
    tabindex="0"
    draggable="true"
    title={describe(s)}
    data-session={s.name}
    ondragstart={(e) => dragStart(e, "s", s.id)}
    onclick={() => (selected = s.id)}
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
      style:padding-left="{6 + depth * 16}px"
      role="treeitem"
      aria-selected="false"
      aria-expanded={isOpen}
      tabindex="0"
      draggable="true"
      data-folder={f.name}
      ondragstart={(e) => dragStart(e, "f", f.id)}
      ondragover={(e) => dragOver(e, f.id)}
      ondragleave={() => dropTarget === f.id && (dropTarget = null)}
      ondrop={(e) => drop(e, f.id)}
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

<div class="tree" role="tree">
  <div
    class="row folder root"
    class:drop={dropTarget === ""}
    role="treeitem"
    aria-selected="false"
    aria-expanded={!rootCollapsed}
    tabindex="0"
    ondragover={(e) => dragOver(e, "")}
    ondragleave={() => dropTarget === "" && (dropTarget = null)}
    ondrop={(e) => drop(e, "")}
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
</div>

<style>
  .tree {
    user-select: none;
    padding: 2px 0;
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
