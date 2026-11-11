<script lang="ts">
  // Comparison tab (like the diff tool of the original): two text files side by side with
  // the differences highlighted (editable, saved back), or two folders.
  import { onMount, untrack } from "svelte";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { MergeView, goToNextChunk, goToPreviousChunk } from "@codemirror/merge";
  import { EditorView, lineNumbers, highlightActiveLine, keymap, drawSelection } from "@codemirror/view";
  import { Compartment, EditorState } from "@codemirror/state";
  import { history, defaultKeymap, historyKeymap } from "@codemirror/commands";
  import { search, searchKeymap } from "@codemirror/search";
  import { api, asApiError, formatSize, type TextFile } from "./api";
  import { app, type Tab } from "./app.svelte";
  import { theme } from "./theme.svelte";
  import { appearance, languageFor } from "./cmtheme";
  import Icon from "./Icon.svelte";

  let { tab }: { tab: Tab } = $props();

  type Status = "same" | "different" | "left_only" | "right_only" | "kind_mismatch";
  type Entry = {
    path: string;
    dir: boolean;
    status: Status;
    left_size: number | null;
    right_size: number | null;
    left_mtime: number | null;
    right_mtime: number | null;
  };

  const init = untrack(() => ({ ...tab.session.extra }));
  let mode: "files" | "dirs" = $state(init.mode === "dirs" ? "dirs" : "files");
  let left = $state(init.left ?? "");
  let right = $state(init.right ?? "");
  let error = $state("");
  let busy = $state(false);
  let chunks = $state(0);
  let dirtyA = $state(false);
  let dirtyB = $state(false);
  let entries: Entry[] = $state([]);
  let truncated = $state(false);
  let hideSame = $state(true);

  let host: HTMLDivElement | undefined = $state();
  let mv: MergeView | undefined = $state.raw();
  let files: [TextFile, TextFile] | null = null;
  let savedA = "";
  let savedB = "";
  const lookA = new Compartment();
  const lookB = new Compartment();

  const LABEL: Record<Status, string> = {
    same: "identique",
    different: "différent",
    left_only: "seulement à gauche",
    right_only: "seulement à droite",
    kind_mismatch: "fichier / dossier",
  };
  const shown = $derived(entries.filter((e) => !hideSame || e.status !== "same"));
  const counts = $derived({
    different: entries.filter((e) => e.status === "different" && !e.dir).length,
    left: entries.filter((e) => e.status === "left_only").length,
    right: entries.filter((e) => e.status === "right_only").length,
  });

  function remember() {
    tab.session.extra = { ...tab.session.extra, mode, left, right };
    const n = (p: string) => p.split(/[\\/]/).filter(Boolean).pop() ?? p;
    tab.title = tab.session.name = left && right ? `${n(left)} ↔ ${n(right)}` : "Comparaison";
    tab.dirty = dirtyA || dirtyB;
  }

  function sideExtensions(look: Compartment, onChange: () => void) {
    return [
      lineNumbers(),
      highlightActiveLine(),
      drawSelection(),
      history(),
      search(),
      keymap.of([...defaultKeymap, ...historyKeymap, ...searchKeymap]),
      look.of(appearance(theme.dark)),
      EditorView.updateListener.of((u) => u.docChanged && onChange()),
    ];
  }

  function countChunks() {
    chunks = mv?.chunks.length ?? 0;
  }

  async function compareFiles() {
    mv?.destroy();
    mv = undefined;
    files = null;
    chunks = 0;
    const [a, b] = await Promise.all([api.textRead("", left), api.textRead("", right)]);
    files = [a, b];
    savedA = a.text;
    savedB = b.text;
    dirtyA = dirtyB = false;
    const [la, lb] = await Promise.all([languageFor(left), languageFor(right)]);
    if (!host) return;
    mv = new MergeView({
      parent: host,
      a: {
        doc: a.text,
        extensions: [
          sideExtensions(lookA, () => {
            dirtyA = mv!.a.state.doc.toString() !== savedA;
            remember();
            countChunks();
          }),
          la.support,
        ],
      },
      b: {
        doc: b.text,
        extensions: [
          sideExtensions(lookB, () => {
            dirtyB = mv!.b.state.doc.toString() !== savedB;
            remember();
            countChunks();
          }),
          lb.support,
        ],
      },
      highlightChanges: true,
      gutter: true,
      revertControls: "a-to-b",
      collapseUnchanged: { margin: 3, minSize: 6 },
    });
    countChunks();
  }

  async function compareDirs() {
    mv?.destroy();
    mv = undefined;
    const r = await api.dirCompare(left, right);
    entries = r.entries;
    truncated = r.truncated;
  }

  async function compare() {
    error = "";
    if (!left || !right) {
      error = "Choisis les deux éléments à comparer.";
      return;
    }
    if ((dirtyA || dirtyB) && !(await app.confirm("Abandonner les modifications non enregistrées ?"))) return;
    busy = true;
    remember();
    try {
      if (mode === "files") await compareFiles();
      else await compareDirs();
    } catch (e) {
      error = asApiError(e).message;
    } finally {
      busy = false;
      remember();
    }
  }

  async function pick(side: "left" | "right") {
    const p = await openDialog({ directory: mode === "dirs", multiple: false, title: side === "left" ? "Élément de gauche" : "Élément de droite" });
    if (!p || Array.isArray(p)) return;
    if (side === "left") left = p;
    else right = p;
    if (left && right) compare();
  }

  function swap() {
    [left, right] = [right, left];
    if (left && right) compare();
  }

  async function save(side: "a" | "b") {
    if (!mv || !files) return;
    const view = side === "a" ? mv.a : mv.b;
    const f = side === "a" ? files[0] : files[1];
    const text = view.state.doc.toString();
    try {
      await api.textWrite("", side === "a" ? left : right, { ...f, text });
      if (side === "a") {
        savedA = text;
        dirtyA = false;
      } else {
        savedB = text;
        dirtyB = false;
      }
      remember();
    } catch (e) {
      error = asApiError(e).message;
    }
  }

  function nav(next: boolean) {
    if (!mv) return;
    const v = mv.b.hasFocus ? mv.b : mv.a;
    (next ? goToNextChunk : goToPreviousChunk)(v);
    v.focus();
  }

  function join(root: string, rel: string) {
    const sep = root.includes("\\") ? "\\" : "/";
    return root.replace(/[\\/]+$/, "") + sep + rel.split("/").join(sep);
  }

  /** Opens two different files of the folder comparison in a new comparison tab. */
  function openPair(e: Entry) {
    if (e.dir || e.status !== "different") return;
    app.openDiff(join(left, e.path), join(right, e.path), "files");
  }

  function fmtDate(t: number | null) {
    return t ? new Date(t * 1000).toLocaleString("fr-FR", { dateStyle: "short", timeStyle: "short" }) : "";
  }

  onMount(() => {
    remember();
    if (left && right) compare();
    return () => mv?.destroy();
  });

  $effect(() => {
    const dark = theme.dark;
    mv?.a.dispatch({ effects: lookA.reconfigure(appearance(dark)) });
    mv?.b.dispatch({ effects: lookB.reconfigure(appearance(dark)) });
  });
</script>

<div class="diff" data-diff={tab.key}>
  <div class="bar">
    <div class="seg" role="radiogroup" aria-label="Type de comparaison">
      <button class:on={mode === "files"} role="radio" aria-checked={mode === "files"} onclick={() => { mode = "files"; entries = []; }}>Fichiers</button>
      <button class:on={mode === "dirs"} role="radio" aria-checked={mode === "dirs"} onclick={() => { mode = "dirs"; mv?.destroy(); mv = undefined; chunks = 0; }}>Dossiers</button>
    </div>
    <form class="paths" onsubmit={(e) => { e.preventDefault(); compare(); }}>
      <input data-left bind:value={left} placeholder={mode === "files" ? "Fichier de gauche" : "Dossier de gauche"} spellcheck="false" />
      <button type="button" title="Choisir…" onclick={() => pick("left")}><Icon name="folder" size={14} /></button>
      <button type="button" title="Échanger gauche et droite" onclick={swap}>⇄</button>
      <input data-right bind:value={right} placeholder={mode === "files" ? "Fichier de droite" : "Dossier de droite"} spellcheck="false" />
      <button type="button" title="Choisir…" onclick={() => pick("right")}><Icon name="folder" size={14} /></button>
      <button type="submit" class="primary" disabled={busy}>{busy ? "Comparaison…" : "Comparer"}</button>
    </form>
  </div>
  {#if error}<div class="msg err">{error}</div>{/if}

  {#if mode === "files"}
    {#if mv || busy}
      <div class="sub">
        <span class="count">{chunks === 0 ? "Aucune différence" : `${chunks} différence${chunks > 1 ? "s" : ""}`}</span>
        <button title="Différence précédente" onclick={() => nav(false)} disabled={!chunks}>▲</button>
        <button title="Différence suivante" onclick={() => nav(true)} disabled={!chunks}>▼</button>
        <span class="grow"></span>
        <button onclick={() => save("a")} disabled={!dirtyA}><Icon name="save" size={14} /> Enregistrer la gauche</button>
        <button onclick={() => save("b")} disabled={!dirtyB}><Icon name="save" size={14} /> Enregistrer la droite</button>
      </div>
    {/if}
    <div class="host" bind:this={host}>
      {#if !mv && !busy}<p class="empty">Choisis deux fichiers texte : les lignes différentes sont surlignées, la flèche entre les deux recopie un bloc de gauche à droite.</p>{/if}
    </div>
  {:else}
    {#if entries.length}
      <div class="sub">
        <span class="count">{counts.different} fichier(s) différent(s), {counts.left} seulement à gauche, {counts.right} seulement à droite{truncated ? " — liste tronquée" : ""}</span>
        <span class="grow"></span>
        <label class="chk"><input type="checkbox" bind:checked={hideSame} /> Masquer les identiques</label>
      </div>
      <div class="table">
        <div class="row head"><span>Chemin</span><span class="r">Gauche</span><span>État</span><span class="r">Droite</span></div>
        {#each shown as e (e.path)}
          <div
            class="row {e.status}"
            class:dir={e.dir}
            role="button"
            tabindex="0"
            title={e.status === "different" && !e.dir ? "Double-clic : comparer les deux fichiers" : ""}
            ondblclick={() => openPair(e)}
            onkeydown={(k) => k.key === "Enter" && openPair(e)}
          >
            <span class="p" style="padding-left: {e.path.split('/').length * 12 - 6}px"><Icon name={e.dir ? "folder" : "file"} size={14} /> {e.path.split("/").pop()}</span>
            <span class="r" title={fmtDate(e.left_mtime)}>{e.left_size !== null ? formatSize(e.left_size) : e.left_mtime !== null ? "dossier" : "—"}</span>
            <span class="st">{LABEL[e.status]}</span>
            <span class="r" title={fmtDate(e.right_mtime)}>{e.right_size !== null ? formatSize(e.right_size) : e.right_mtime !== null ? "dossier" : "—"}</span>
          </div>
        {:else}
          <p class="empty">Les deux dossiers sont identiques.</p>
        {/each}
      </div>
    {:else}
      <div class="host"><p class="empty">Choisis deux dossiers : les fichiers différents ou présents d’un seul côté sont listés (double-clic pour comparer deux fichiers).</p></div>
    {/if}
  {/if}
</div>

<style>
  .diff {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    background: var(--panel);
    font-size: 12.5px;
  }
  .bar,
  .sub {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 6px;
    background: var(--chrome);
    border-bottom: 1px solid var(--chrome-border);
    min-width: 0;
  }
  .seg {
    display: inline-flex;
  }
  .seg button {
    border-radius: 0;
    padding: 2px 10px;
  }
  .seg button:first-child {
    border-radius: 4px 0 0 4px;
  }
  .seg button:last-child {
    border-radius: 0 4px 4px 0;
    margin-left: -1px;
  }
  .seg button.on,
  button.on {
    background: var(--selected);
    border-color: var(--hover-border);
  }
  .paths {
    flex: 1;
    display: flex;
    gap: 4px;
    min-width: 0;
  }
  .paths input {
    flex: 1;
    min-width: 0;
    font-family: Consolas, monospace;
    font-size: 12px;
  }
  .bar button,
  .sub button {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px 8px;
    font-size: 12.5px;
  }
  .grow {
    flex: 1;
  }
  .count {
    color: var(--text);
  }
  .chk {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .msg.err {
    padding: 5px 10px;
    background: color-mix(in srgb, var(--danger) 12%, transparent);
    color: var(--danger);
  }
  .host {
    flex: 1;
    min-height: 0;
    overflow: auto;
    background: var(--input-bg);
  }
  .host :global(.cm-mergeView) {
    height: 100%;
  }
  .host :global(.cm-mergeViewEditors) {
    min-height: 100%;
  }
  .host :global(.cm-editor) {
    height: auto;
  }
  .host :global(.cm-editor.cm-focused) {
    outline: none;
  }
  .empty {
    color: var(--muted);
    padding: 24px;
    text-align: center;
  }
  .table {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
  .row {
    display: grid;
    grid-template-columns: minmax(200px, 1fr) 90px 150px 90px;
    gap: 8px;
    padding: 3px 10px;
    border-bottom: 1px solid var(--row-border);
    align-items: center;
    cursor: default;
  }
  .row.head {
    position: sticky;
    top: 0;
    background: var(--chrome);
    color: var(--muted);
    font-weight: 600;
  }
  .row .p {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .r {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .row.different .st {
    color: var(--warn);
    font-weight: 600;
  }
  .row.different:not(.dir) {
    cursor: pointer;
  }
  .row.left_only .st,
  .row.right_only .st,
  .row.kind_mismatch .st {
    color: var(--danger);
  }
  .row.same .st {
    color: var(--muted);
  }
  .row:hover {
    background: var(--hover);
  }
</style>
