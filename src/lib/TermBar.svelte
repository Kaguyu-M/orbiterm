<script lang="ts">
  // Bar above remote terminals (SSH, Telnet…), like the one of RDP desktops:
  // copy / paste, clear, separate SFTP session, full screen, close the session.
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import Icon from "./Icon.svelte";
  import { protocol } from "./api";
  import { app, type Tab } from "./app.svelte";
  import { terminals } from "./Terminal.svelte";

  let { tab }: { tab: Tab } = $props();

  const open = $derived(tab.state === "open" && !!tab.connId);
  const ctl = () => terminals.get(tab.key);
  let full = $state(false);

  function run(f: (c: NonNullable<ReturnType<typeof ctl>>) => void) {
    const c = ctl();
    if (!c) return;
    f(c);
    c.focus();
  }

  async function toggleFull() {
    const w = getCurrentWindow();
    full = !(await w.isFullscreen().catch(() => false));
    await w.setFullscreen(full).catch(() => {});
    ctl()?.focus();
  }

  async function closeSession() {
    const what = protocol(tab.session.kind).label;
    if (!(await app.confirm(`Fermer la session ${what} « ${tab.title} » ? La connexion au serveur sera coupée.`))) return;
    await app.closeTab(tab);
  }
</script>

<div class="tbar" data-termbar>
  <Icon name={protocol(tab.session.kind).icon} size={16} />
  <span class="title">{tab.title}</span>
  <span class="st" class:ok={open}>{open ? "● connecté" : tab.state === "connecting" ? "connexion…" : "○ terminé"}</span>
  <button title="Copier la sélection (Ctrl+Maj+C)" onclick={() => run((c) => c.copy())}><Icon name="copy" size={14} /> Copier</button>
  <button title="Coller le presse-papiers (Ctrl+Maj+V, Maj+Inser, clic droit)" disabled={!open} onclick={() => run((c) => c.paste())}>
    <Icon name="download" size={14} /> Coller
  </button>
  <button title="Effacer l'écran et l'historique du terminal" onclick={() => run((c) => c.clear())}><Icon name="trash" size={14} /> Effacer</button>
  {#if tab.session.kind === "ssh"}
    <button title="Ouvrir ce serveur dans une session SFTP séparée (enregistrée, mot de passe mémorisé)" onclick={() => app.openSeparateSftp(tab)}>
      <Icon name="sftp" size={14} /> Session SFTP
    </button>
  {/if}
  <button title={full ? "Quitter le plein écran (F11)" : "Plein écran (F11)"} onclick={toggleFull}>{full ? "Quitter le plein écran" : "Plein écran"}</button>
  <button class="end" data-term-end disabled={!open} title="Fermer la session distante et l'onglet" onclick={closeSession}>
    <Icon name="exit" size={14} /> Fermer la session
  </button>
</div>

<style>
  .tbar {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 30px;
    padding: 0 8px;
    background: var(--chrome);
    border-bottom: 1px solid var(--chrome-border);
    white-space: nowrap;
    overflow: hidden;
    user-select: none;
  }
  .tbar button {
    padding: 1px 8px;
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .title {
    font-weight: 600;
    max-width: 260px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .st {
    color: var(--muted);
    font-size: 11.5px;
    margin-right: 4px;
  }
  .st.ok {
    color: var(--ok);
  }
  .tbar .end {
    margin-left: auto;
    background: var(--danger);
    border-color: var(--danger);
    color: #fff;
  }
  .tbar .end:hover:not(:disabled) {
    filter: brightness(1.1);
  }
</style>
