<script lang="ts">
  // Text editor tab (like the editor built into the original): local files, or remote
  // files opened from the SFTP / FTP browser and sent back to the server on save.
  import { onMount } from "svelte";
  import { save as saveDialog } from "@tauri-apps/plugin-dialog";
  import { basicSetup } from "codemirror";
  import { EditorView, keymap } from "@codemirror/view";
  import { Compartment, EditorState } from "@codemirror/state";
  import { openSearchPanel, gotoLine } from "@codemirror/search";
  import { indentWithTab } from "@codemirror/commands";
  import { api, asApiError, type TextFile } from "./api";
  import { app, type Tab } from "./app.svelte";
  import { theme } from "./theme.svelte";
  import Icon from "./Icon.svelte";
  import { appearance, languageFor } from "./cmtheme";

  let { tab, visible }: { tab: Tab; visible: boolean } = $props();

  let host: HTMLDivElement;
  let view: EditorView | undefined;
  let file: TextFile = $state({ text: "", encoding: "utf8", crlf: false });
  let saved = ""; // text as last loaded / saved
  let loading = $state(true);
  let saving = $state(false);
  let error = $state("");
  let info = $state("");
  let pos = $state({ line: 1, col: 1, sel: 0 });
  let langName = $state("Texte");
  let wrap = $state(false);

  const lang = new Compartment();
  const wrapC = new Compartment();
  const look = new Compartment();

  const path = $derived(tab.session.extra?.path ?? "");
  const conn = $derived(tab.session.extra?.conn ?? "");
  const source = $derived(tab.session.extra?.source ?? "");
  const ENC: Record<TextFile["encoding"], string> = { utf8: "UTF-8", "utf8-bom": "UTF-8 (BOM)", utf16le: "UTF-16 LE", latin1: "Latin-1" };

  function current(): string {
    return view?.state.doc.toString() ?? file.text;
  }

  function markDirty() {
    tab.dirty = current() !== saved;
  }

  async function detectLanguage(name: string) {
    const l = await languageFor(name);
    langName = l.name;
    view?.dispatch({ effects: lang.reconfigure(l.support) });
  }

  function setDoc(text: string) {
    if (!view) return;
    view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: text }, selection: { anchor: 0 } });
    view.scrollDOM.scrollTop = 0;
  }

  async function load() {
    error = "";
    info = "";
    if (!path) {
      loading = false;
      return;
    }
    loading = true;
    try {
      file = await api.textRead(conn, path);
      saved = file.text;
      setDoc(file.text);
      tab.dirty = false;
      await detectLanguage(path.split(/[\\/]/).pop() ?? path);
    } catch (e) {
      error = asApiError(e).message;
    } finally {
      loading = false;
    }
  }

  async function reload() {
    if (tab.dirty && !(await app.confirm("Abandonner les modifications et relire le fichier ?"))) return;
    await load();
  }

  /** Writes the file (asks for a local path for a new file or "save as"). */
  async function save(as = false): Promise<boolean> {
    if (saving) return false;
    let target = path;
    let targetConn = conn;
    if (as || !target) {
      const name = target.split(/[\\/]/).pop() || "nouveau.txt";
      const picked = await saveDialog({ defaultPath: conn ? name : target || name, title: "Enregistrer sous" });
      if (!picked) return false;
      target = picked;
      targetConn = "";
    }
    saving = true;
    error = "";
    try {
      const text = current();
      await api.textWrite(targetConn, target, { ...file, text });
      file.text = text;
      saved = text;
      tab.dirty = false;
      if (target !== path || targetConn !== conn) {
        tab.session.extra = { ...tab.session.extra, path: target, conn: targetConn, source: "" };
        tab.title = tab.session.name = target.split(/[\\/]/).pop() || target;
        await detectLanguage(tab.title);
      }
      info = targetConn ? `Enregistré et envoyé sur le serveur (${source || "distant"})` : "Enregistré";
      setTimeout(() => (info = ""), 2500);
      return true;
    } catch (e) {
      const m = asApiError(e).message;
      error = targetConn
        ? `Échec de l'envoi sur le serveur : ${m}. « Enregistrer sous » garde une copie locale.`
        : `Échec de l'enregistrement : ${m}`;
      return false;
    } finally {
      saving = false;
    }
  }

  function setEol(crlf: boolean) {
    file.crlf = crlf;
    tab.dirty = true;
  }

  function toggleWrap() {
    wrap = !wrap;
    view?.dispatch({ effects: wrapC.reconfigure(wrap ? EditorView.lineWrapping : []) });
    view?.focus();
  }

  function run(cmd: (v: EditorView) => boolean) {
    if (view) {
      cmd(view);
      view.focus();
    }
  }

  onMount(() => {
    view = new EditorView({
      parent: host,
      state: EditorState.create({
        doc: "",
        extensions: [
          basicSetup,
          keymap.of([
            indentWithTab,
            { key: "Mod-s", preventDefault: true, run: () => (void save(), true) },
            { key: "Mod-Shift-s", preventDefault: true, run: () => (void save(true), true) },
            { key: "Mod-g", preventDefault: true, run: gotoLine },
          ]),
          lang.of([]),
          wrapC.of([]),
          look.of(appearance(theme.dark)),
          EditorView.updateListener.of((u) => {
            if (u.docChanged) markDirty();
            if (u.docChanged || u.selectionSet) {
              const h = u.state.selection.main;
              const l = u.state.doc.lineAt(h.head);
              pos = { line: l.number, col: h.head - l.from + 1, sel: Math.abs(h.to - h.from) };
            }
          }),
        ],
      }),
    });
    load();
    return () => view?.destroy();
  });

  // follows the light / dark interface theme
  $effect(() => {
    const dark = theme.dark;
    view?.dispatch({ effects: look.reconfigure(appearance(dark)) });
  });

  $effect(() => {
    if (visible) queueMicrotask(() => view?.focus());
  });
</script>

<div class="editor" data-editor={tab.key}>
  <div class="bar">
    <button title="Enregistrer (Ctrl+S)" onclick={() => save()} disabled={saving || loading}><Icon name="save" size={15} /> Enregistrer</button>
    <button title="Enregistrer sous… (Ctrl+Maj+S) — fichier local" onclick={() => save(true)} disabled={saving || loading}>Enregistrer sous…</button>
    <button title="Relire le fichier" onclick={reload} disabled={!path || loading}><Icon name="refresh" size={15} /></button>
    <span class="sep"></span>
    <button title="Rechercher / remplacer (Ctrl+F)" onclick={() => run(openSearchPanel)}><Icon name="search" size={15} /></button>
    <button title="Aller à la ligne (Ctrl+G)" onclick={() => run(gotoLine)}>Ligne…</button>
    <button title="Retour à la ligne automatique" class:on={wrap} onclick={toggleWrap}>Retour à la ligne</button>
    <span class="where" title={path}>
      {#if conn}<Icon name="sftp" size={13} /> {source ? `${source} : ` : ""}{:else if path}<Icon name="folder" size={13} />{/if}
      {path || "Nouveau fichier (pas encore enregistré)"}
    </span>
  </div>
  {#if error}<div class="msg err">{error}</div>{/if}
  <div class="host" class:busy={loading} bind:this={host}></div>
  <div class="status">
    <span>Ligne {pos.line}, col. {pos.col}{pos.sel ? ` (${pos.sel} sélectionné${pos.sel > 1 ? "s" : ""})` : ""}</span>
    <span class="grow">{loading ? "Lecture…" : saving ? "Enregistrement…" : info || (tab.dirty ? "Modifié" : "")}</span>
    <span>{langName}</span>
    <span title="Encodage du fichier (conservé à l'enregistrement)">{ENC[file.encoding]}</span>
    <select title="Fins de ligne" value={file.crlf ? "crlf" : "lf"} onchange={(e) => setEol(e.currentTarget.value === "crlf")}>
      <option value="lf">LF (Unix)</option>
      <option value="crlf">CRLF (Windows)</option>
    </select>
  </div>
</div>

<style>
  .editor {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    height: 100%;
    background: var(--panel);
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px 6px;
    border-bottom: 1px solid var(--chrome-border);
    background: var(--chrome);
    font-size: 12.5px;
    min-width: 0;
  }
  .bar button {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px 8px;
    font-size: 12.5px;
  }
  .bar button.on {
    background: var(--selected);
    border-color: var(--hover-border);
  }
  .sep {
    width: 1px;
    align-self: stretch;
    background: var(--chrome-border);
    margin: 0 4px;
  }
  .where {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: right;
    color: var(--muted);
    display: inline-flex;
    justify-content: flex-end;
    align-items: center;
    gap: 4px;
  }
  .msg {
    padding: 5px 10px;
    font-size: 12.5px;
  }
  .msg.err {
    background: color-mix(in srgb, var(--danger) 12%, transparent);
    color: var(--danger);
    border-bottom: 1px solid color-mix(in srgb, var(--danger) 30%, transparent);
  }
  .host {
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }
  .host.busy {
    opacity: 0.5;
  }
  .host :global(.cm-editor) {
    height: 100%;
  }
  .host :global(.cm-editor.cm-focused) {
    outline: none;
  }
  .status {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 2px 10px;
    border-top: 1px solid var(--chrome-border);
    background: var(--chrome);
    color: var(--muted);
    font-size: 12px;
    white-space: nowrap;
  }
  .status .grow {
    flex: 1;
    color: var(--text);
  }
  .status select {
    font-size: 12px;
    padding: 0 2px;
  }
</style>
