<script lang="ts">
  import type { Folder, Session, Store } from "./api";

  let {
    store,
    filter = "",
    onopen,
    onedit,
    onnewsession,
    onnewfolder,
    onrenamefolder,
    ondeletesession,
    ondeletefolder,
    onduplicate,
  }: {
    store: Store;
    filter?: string;
    onopen: (s: Session) => void;
    onedit: (s: Session) => void;
    onnewsession: (folderId: string) => void;
    onnewfolder: (parentId: string) => void;
    onrenamefolder: (f: Folder) => void;
    ondeletesession: (s: Session) => void;
    ondeletefolder: (f: Folder) => void;
    onduplicate: (s: Session) => void;
  } = $props();

  let collapsed: Record<string, boolean> = $state({});

  const byName = (a: { name: string }, b: { name: string }) =>
    a.name.localeCompare(b.name, undefined, { sensitivity: "base" });

  function matches(s: Session) {
    const f = filter.trim().toLowerCase();
    return !f || `${s.name} ${s.host} ${s.username}`.toLowerCase().includes(f);
  }

  function childFolders(parent: string) {
    return store.folders.filter((f) => f.parent === parent).sort(byName);
  }
  function childSessions(folder: string) {
    return store.sessions.filter((s) => s.folder === folder && matches(s)).sort(byName);
  }
  /** Hide empty folders while filtering. */
  function folderHasMatch(id: string): boolean {
    if (!filter.trim()) return true;
    return childSessions(id).length > 0 || childFolders(id).some((f) => folderHasMatch(f.id));
  }
</script>

{#snippet sessionRow(s: Session, depth: number)}
  <div
    class="row session"
    style:padding-left="{8 + depth * 14}px"
    role="button"
    tabindex="0"
    title="{s.username ? s.username + '@' : ''}{s.host}:{s.port}"
    ondblclick={() => onopen(s)}
    onkeydown={(e) => e.key === "Enter" && onopen(s)}
  >
    <span class="spacer"></span>
    <span class="icon">⌘</span>
    <span class="name">{s.name}</span>
    <span class="actions">
      <button title="Ouvrir" onclick={() => onopen(s)}>▶</button>
      <button title="Modifier" onclick={() => onedit(s)}>✎</button>
      <button title="Dupliquer" onclick={() => onduplicate(s)}>⧉</button>
      <button title="Supprimer" onclick={() => ondeletesession(s)}>✕</button>
    </span>
  </div>
{/snippet}

{#snippet folderNode(f: Folder, depth: number)}
  {#if folderHasMatch(f.id)}
    <div class="row folder" style:padding-left="{8 + depth * 14}px">
      <button class="toggle" onclick={() => (collapsed[f.id] = !collapsed[f.id])}>
        {collapsed[f.id] && !filter ? "▸" : "▾"}
      </button>
      <span class="icon">📁</span>
      <span class="name">{f.name}</span>
      <span class="actions">
        <button title="Nouvelle session ici" onclick={() => onnewsession(f.id)}>＋</button>
        <button title="Nouveau sous-dossier" onclick={() => onnewfolder(f.id)}>📁</button>
        <button title="Renommer" onclick={() => onrenamefolder(f)}>✎</button>
        <button title="Supprimer le dossier et son contenu" onclick={() => ondeletefolder(f)}>✕</button>
      </span>
    </div>
    {#if !collapsed[f.id] || filter}
      {#each childFolders(f.id) as sub (sub.id)}
        {@render folderNode(sub, depth + 1)}
      {/each}
      {#each childSessions(f.id) as s (s.id)}
        {@render sessionRow(s, depth + 1)}
      {/each}
    {/if}
  {/if}
{/snippet}

<div class="tree">
  {#each childFolders("") as f (f.id)}
    {@render folderNode(f, 0)}
  {/each}
  {#each childSessions("") as s (s.id)}
    {@render sessionRow(s, 0)}
  {/each}
  {#if store.sessions.length === 0}
    <p class="empty">Aucune session.<br />Clique sur <b>＋ Session</b> pour commencer.</p>
  {/if}
</div>

<style>
  .tree {
    font-size: 13px;
    user-select: none;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    padding-right: 4px;
    cursor: default;
    border-radius: 4px;
  }
  .row:hover,
  .row:focus-visible {
    background: var(--hover);
    outline: none;
  }
  .session {
    cursor: pointer;
  }
  .name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .icon {
    width: 16px;
    text-align: center;
    color: var(--accent);
  }
  .spacer {
    width: 16px;
    flex: 0 0 16px;
  }
  .toggle {
    width: 16px;
    padding: 0;
    border: none;
    background: none;
    color: var(--muted);
  }
  .actions {
    display: none;
    gap: 2px;
  }
  .row:hover .actions,
  .row:focus-within .actions {
    display: flex;
  }
  .actions button {
    padding: 0 5px;
    height: 20px;
    font-size: 11px;
    background: transparent;
    border: none;
    color: var(--muted);
  }
  .actions button:hover {
    color: var(--fg);
    background: var(--border);
  }
  .empty {
    color: var(--muted);
    padding: 16px 8px;
    line-height: 1.6;
  }
</style>
