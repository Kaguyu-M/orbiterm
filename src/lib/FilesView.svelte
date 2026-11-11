<script lang="ts">
  // SFTP / FTP tab: the explorer of this computer (left, can be hidden) next to the remote
  // files (right). Files go from one side to the other by dragging them, or with the
  // "Envoyer" / "Télécharger" buttons.
  import type { Entry, LocalEntry } from "./api";
  import { app } from "./app.svelte";
  import Icon from "./Icon.svelte";
  import LocalPanel from "./LocalPanel.svelte";
  import SftpPanel from "./SftpPanel.svelte";

  let { connId, visible, source = "" }: { connId: string; visible: boolean; source?: string } = $props();

  let local: ReturnType<typeof LocalPanel> | undefined = $state();
  let remote: ReturnType<typeof SftpPanel> | undefined = $state();
  let host: HTMLDivElement;

  const open = $derived(app.prefs.localPane);
  function toggle() {
    app.prefs.localPane = !app.prefs.localPane;
    app.savePrefs();
  }

  // ---- drag between the two panes. The native drop handler of the window (files dragged
  // from the system) disables HTML5 drag & drop in the page: this one is made by hand.
  type Side = "local" | "remote";
  type Drag = { from: Side; label: string; run: () => void; x: number; y: number; over: Side | null };
  let drag: Drag | null = $state(null);

  function sideAt(x: number, y: number): Side | null {
    const el = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-drop]");
    return el && host.contains(el) ? (el.dataset.drop as Side) : null;
  }

  function begin(ev: PointerEvent, from: Side, label: string, run: () => void) {
    if (ev.button !== 0) return;
    const sx = ev.clientX;
    const sy = ev.clientY;
    const move = (m: PointerEvent) => {
      if (!drag && Math.hypot(m.clientX - sx, m.clientY - sy) < 6) return;
      drag = { from, label, run, x: m.clientX, y: m.clientY, over: sideAt(m.clientX, m.clientY) };
    };
    const end = (u: PointerEvent) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", end);
      window.removeEventListener("pointercancel", end);
      const d = drag;
      drag = null;
      if (d && u.type === "pointerup" && sideAt(u.clientX, u.clientY) === (d.from === "local" ? "remote" : "local")) d.run();
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", end);
    window.addEventListener("pointercancel", end);
  }

  const dragLocal = (ev: PointerEvent, e: LocalEntry) => begin(ev, "local", `⬆ ${e.name}`, () => remote?.uploadFiles([e.path]));
  const dragRemote = (ev: PointerEvent, e: Entry) => begin(ev, "remote", `⬇ ${e.name}`, () => remote?.download(e));

  // ---- width of the local pane
  function resize(ev: PointerEvent) {
    ev.preventDefault();
    const left = host.getBoundingClientRect().left;
    const total = host.clientWidth;
    const move = (m: PointerEvent) => {
      app.prefs.localPaneWidth = Math.round(Math.min(Math.max(m.clientX - left, 260), Math.max(260, total - 320)));
    };
    const end = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", end);
      app.savePrefs();
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", end);
  }
</script>

<div class="files" class:dragging={!!drag} bind:this={host} data-files-view>
  {#if open}
    <div class="pane left" style:width="{app.prefs.localPaneWidth}px">
      <LocalPanel bind:this={local} {visible} onsend={(paths) => remote?.uploadFiles(paths)} onitemdrag={dragLocal} onclose={toggle} dropActive={drag?.from === "remote" && drag.over === "local"} />
    </div>
    <div class="split" role="separator" aria-orientation="vertical" aria-label="Largeur du volet local" onpointerdown={resize}></div>
  {:else}
    <button class="reopen" title="Afficher le volet du poste local" aria-label="Afficher le volet du poste local" onclick={toggle} data-local-open>
      <Icon name="shell" size={14} /><span>Poste local</span>
    </button>
  {/if}
  <div class="pane right" data-drop="remote">
    <SftpPanel
      bind:this={remote}
      wide
      {connId}
      {visible}
      askText={app.askText}
      confirm={app.confirm}
      {source}
      localDir={() => (open ? (local?.dir() ?? null) : null)}
      onlocalchange={() => local?.refresh()}
      onitemdrag={open ? dragRemote : undefined}
      dropActive={drag?.from === "local" && drag.over === "remote"}
    />
  </div>
  {#if drag}
    <div class="ghost" class:ok={drag.over !== null && drag.over !== drag.from} style:left="{drag.x + 12}px" style:top="{drag.y + 14}px">{drag.label}</div>
  {/if}
</div>

<style>
  .files {
    position: absolute;
    inset: 0;
    display: flex;
    background: var(--panel);
  }
  .files.dragging {
    cursor: grabbing;
    user-select: none;
  }
  .pane {
    position: relative;
    min-width: 0;
  }
  .left {
    flex: none;
  }
  .right {
    flex: 1;
  }
  .split {
    flex: none;
    width: 5px;
    cursor: col-resize;
    background: var(--chrome-border);
  }
  .split:hover {
    background: var(--accent);
  }
  .reopen {
    flex: none;
    width: 24px;
    padding: 8px 0;
    border: none;
    border-right: 1px solid var(--chrome-border);
    border-radius: 0;
    background: var(--chrome);
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: flex-start;
    gap: 6px;
    color: var(--muted);
  }
  .reopen span {
    writing-mode: vertical-rl;
    transform: rotate(180deg);
    font-size: 12px;
  }
  .reopen:hover {
    background: var(--hover);
    color: var(--text);
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
</style>
