<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import {
    api,
    asApiError,
    newSession,
    type Folder,
    type HostKeyPrompt,
    type Session,
    type Store,
  } from "$lib/api";
  import { startBus, onConnectionClosed } from "$lib/bus";
  import Modal from "$lib/Modal.svelte";
  import SessionDialog from "$lib/SessionDialog.svelte";
  import SessionTree from "$lib/SessionTree.svelte";
  import SftpPanel from "$lib/SftpPanel.svelte";
  import Terminal from "$lib/Terminal.svelte";

  // ------------------------------------------------------------ state
  let store: Store = $state({ version: 1, folders: [], sessions: [] });
  let filter = $state("");
  let sideTab: "sessions" | "sftp" = $state("sessions");
  let sidebarWidth = $state(300);

  type Tab = {
    key: string;
    session: Session;
    connId: string | null;
    status: string;
    state: "connecting" | "open" | "closed" | "error";
  };
  let tabs: Tab[] = $state([]);
  let activeKey: string | null = $state(null);
  const active = $derived(tabs.find((t) => t.key === activeKey) ?? null);
  /** The SFTP side tab exists only while the active terminal has a live connection. */
  const hasSftp = $derived(!!active && active.state === "open" && !!active.connId && active.session.sftp_enabled);
  $effect(() => {
    if (!hasSftp && sideTab === "sftp") sideTab = "sessions";
  });

  let editing: Session | null = $state(null);

  // generic dialogs (promise based)
  type TextAsk = { title: string; value: string; secret: boolean; check?: string; checked: boolean; resolve: (v: { value: string; checked: boolean } | null) => void };
  let textAsk: TextAsk | null = $state(null);
  type ConfirmAsk = { message: string; resolve: (v: boolean) => void };
  let confirmAsk: ConfirmAsk | null = $state(null);
  let hostKeys: HostKeyPrompt[] = $state([]);

  function ask(title: string, initial = "", secret = false, check?: string, checked = false) {
    return new Promise<{ value: string; checked: boolean } | null>((resolve) => {
      textAsk = { title, value: initial, secret, check, checked, resolve };
    });
  }
  function closeAsk(ok: boolean) {
    if (!textAsk) return;
    const t = textAsk;
    textAsk = null;
    t.resolve(ok ? { value: t.value, checked: t.checked } : null);
  }
  async function askText(title: string, initial: string) {
    const r = await ask(title, initial);
    return r?.value.trim() || null;
  }
  function confirmBox(message: string) {
    return new Promise<boolean>((resolve) => (confirmAsk = { message, resolve }));
  }
  function closeConfirm(v: boolean) {
    const c = confirmAsk;
    confirmAsk = null;
    c?.resolve(v);
  }

  // ------------------------------------------------------------ sessions
  async function reload() {
    store = await api.sessionsLoad();
  }

  async function saveSession(s: Session) {
    editing = null;
    try {
      await api.sessionSave(s);
      await reload();
    } catch (e) {
      alertMsg(asApiError(e).message);
    }
  }

  async function duplicate(s: Session) {
    const copy = structuredClone($state.snapshot(s)) as Session;
    copy.id = "";
    copy.name = `${s.name} (copie)`;
    await saveSession(copy);
  }

  async function deleteSession(s: Session) {
    if (!(await confirmBox(`Supprimer la session « ${s.name} » ?`))) return;
    await api.sessionDelete(s.id);
    await reload();
  }

  async function newFolder(parent: string) {
    const name = await askText("Nom du dossier", "");
    if (!name) return;
    await api.folderSave({ id: "", name, parent });
    await reload();
  }

  async function renameFolder(f: Folder) {
    const name = await askText("Renommer le dossier", f.name);
    if (!name) return;
    await api.folderSave({ ...f, name });
    await reload();
  }

  async function deleteFolder(f: Folder) {
    if (!(await confirmBox(`Supprimer le dossier « ${f.name} », ses sous-dossiers et toutes leurs sessions ?`))) return;
    await api.folderDelete(f.id);
    await reload();
  }

  let alertText = $state("");
  function alertMsg(m: string) {
    alertText = m;
  }

  // ------------------------------------------------------------ connections
  function openSession(s: Session) {
    const tab: Tab = {
      key: crypto.randomUUID(),
      session: structuredClone($state.snapshot(s)) as Session,
      connId: null,
      status: "",
      state: "connecting",
    };
    tabs.push(tab);
    activeKey = tab.key;
    connect(tabs[tabs.length - 1]);
  }

  async function connect(
    tab: Tab,
    extra: { password?: string; passphrase?: string; remember?: boolean } = {},
    note = "",
  ): Promise<void> {
    const s = tab.session;
    tab.state = "connecting";
    tab.status = `Connexion à ${s.username ? s.username + "@" : ""}${s.host}:${s.port}…`;
    try {
      const connId = await api.sshConnect({ session_id: s.id, cols: 120, rows: 32, ...extra });
      tab.connId = connId;
      tab.state = "open";
      tab.status = "";
      if (s.sftp_enabled && tab.key === activeKey) sideTab = "sftp";
    } catch (e) {
      const err = asApiError(e);
      if (err.kind === "need_password") {
        const r = await ask(
          `${note}Mot de passe pour ${s.username}@${s.host}`,
          "",
          true,
          s.remember_password ? "Mémoriser dans le trousseau du système" : undefined,
          s.remember_password,
        );
        if (r) return connect(tab, { password: r.value, remember: r.checked });
        tab.state = "error";
        tab.status = "Connexion annulée.";
      } else if (err.kind === "need_passphrase") {
        const r = await ask(`${note}Passphrase de la clé ${s.key_path}`, "", true);
        if (r) return connect(tab, { passphrase: r.value });
        tab.state = "error";
        tab.status = "Connexion annulée.";
      } else if (err.kind === "auth_failed" && extra.password !== undefined) {
        return connect(tab, {}, "⚠ Mot de passe refusé — ");
      } else if (err.kind === "auth_failed" && extra.passphrase !== undefined) {
        return connect(tab, {}, "⚠ Clé refusée — ");
      } else if (err.kind === "ssh" && extra.passphrase !== undefined && /decrypt|passphrase|crypto/i.test(err.message)) {
        return connect(tab, {}, "⚠ Passphrase incorrecte — ");
      } else {
        tab.state = "error";
        tab.status = `Échec : ${err.message}`;
      }
    }
  }

  function reconnect(tab: Tab) {
    const old = tab.connId;
    tab.connId = null;
    if (old) api.sshDisconnect(old).catch(() => {});
    connect(tab);
  }

  async function closeTab(tab: Tab) {
    if (tab.connId) await api.sshDisconnect(tab.connId).catch(() => {});
    const i = tabs.findIndex((t) => t.key === tab.key);
    tabs.splice(i, 1);
    if (activeKey === tab.key) activeKey = tabs[Math.min(i, tabs.length - 1)]?.key ?? null;
  }

  async function answerHostKey(accept: boolean) {
    const hk = hostKeys.shift();
    if (hk) await api.hostkeyAnswer(hk.request_id, accept);
  }

  // ------------------------------------------------------------ sidebar resize
  function startResize(e: PointerEvent) {
    const startX = e.clientX;
    const startW = sidebarWidth;
    const move = (ev: PointerEvent) => (sidebarWidth = Math.min(600, Math.max(200, startW + ev.clientX - startX)));
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  function keys(e: KeyboardEvent) {
    if (e.ctrlKey && e.shiftKey && e.code === "KeyW" && active) {
      e.preventDefault();
      closeTab(active);
    }
    if (e.ctrlKey && e.code === "Tab" && tabs.length > 1) {
      e.preventDefault();
      const i = tabs.findIndex((t) => t.key === activeKey);
      activeKey = tabs[(i + (e.shiftKey ? tabs.length - 1 : 1)) % tabs.length].key;
    }
  }

  onMount(() => {
    const unsubs: Array<() => void> = [];
    startBus();
    reload().catch((e) => alertMsg(asApiError(e).message));
    listen<HostKeyPrompt>("hostkey-prompt", (e) => hostKeys.push(e.payload)).then((u) => unsubs.push(u));
    unsubs.push(
      onConnectionClosed((connId, reason) => {
        const t = tabs.find((x) => x.connId === connId);
        if (t) {
          // the terminal already prints the reason; release the backend connection
          t.state = "closed";
          api.sshDisconnect(connId).catch(() => {});
        }
      }),
    );
    return () => unsubs.forEach((u) => u());
  });
</script>

<svelte:window onkeydown={keys} />

<div class="app" style:--sidebar="{sidebarWidth}px">
  <aside>
    <nav class="sidetabs">
      <button class:on={sideTab === "sessions"} onclick={() => (sideTab = "sessions")}>Sessions</button>
      {#if hasSftp}
        <button class:on={sideTab === "sftp"} onclick={() => (sideTab = "sftp")}>SFTP</button>
      {/if}
    </nav>

    <div class="sidebody" class:hide={sideTab !== "sessions"}>
      <div class="sessbar">
        <button class="primary" onclick={() => (editing = newSession())}>＋ Session</button>
        <button onclick={() => newFolder("")}>＋ Dossier</button>
      </div>
      <input class="search" placeholder="Rechercher…" bind:value={filter} />
      <div class="scroll">
        <SessionTree
          {store}
          {filter}
          onopen={openSession}
          onedit={(s) => (editing = s)}
          onnewsession={(f) => (editing = newSession(f))}
          onnewfolder={newFolder}
          onrenamefolder={renameFolder}
          ondeletesession={deleteSession}
          ondeletefolder={deleteFolder}
          onduplicate={duplicate}
        />
      </div>
    </div>

    <div class="sidebody" class:hide={sideTab !== "sftp"}>
      {#each tabs as t (t.key)}
        {#if t.connId && t.state === "open" && t.session.sftp_enabled}
          <div class="fill" class:hide={t.key !== activeKey}>
            <SftpPanel
              connId={t.connId}
              visible={sideTab === "sftp" && t.key === activeKey}
              {askText}
              confirm={confirmBox}
            />
          </div>
        {/if}
      {/each}
    </div>
  </aside>

  <div class="splitter" role="separator" aria-orientation="vertical" onpointerdown={startResize}></div>

  <main>
    <div class="tabbar">
      {#each tabs as t (t.key)}
        <div
          class="tab"
          class:on={t.key === activeKey}
          role="tab"
          tabindex="0"
          aria-selected={t.key === activeKey}
          onclick={() => (activeKey = t.key)}
          onkeydown={(e) => e.key === "Enter" && (activeKey = t.key)}
          onauxclick={(e) => e.button === 1 && closeTab(t)}
        >
          <span class="dot {t.state}"></span>
          <span class="tname">{t.session.name}</span>
          <button class="x" title="Fermer (Ctrl+Maj+W)" onclick={(e) => { e.stopPropagation(); closeTab(t); }}>✕</button>
        </div>
      {/each}
    </div>

    <div class="terms">
      {#each tabs as t (t.key)}
        <div class="termwrap" class:hide={t.key !== activeKey} data-conn={t.connId ?? ""}>
          <Terminal connId={t.connId} settings={t.session.terminal} active={t.key === activeKey} status={t.status} />
          {#if t.state === "closed" || t.state === "error"}
            <div class="overlay">
              <button class="primary" onclick={() => reconnect(t)}>⟳ Reconnecter</button>
              <button onclick={() => closeTab(t)}>Fermer l'onglet</button>
            </div>
          {/if}
        </div>
      {:else}
        <div class="welcome">
          <h1>Orbiterm</h1>
          <p>Double-clique sur une session à gauche, ou crée-en une avec <b>＋ Session</b>.</p>
          <p class="muted">Ctrl+Maj+C / Ctrl+Maj+V : copier / coller · Ctrl+Tab : onglet suivant · Ctrl+Maj+W : fermer</p>
        </div>
      {/each}
    </div>
  </main>
</div>

{#if editing}
  <SessionDialog session={editing} folders={store.folders} onsave={saveSession} oncancel={() => (editing = null)} />
{/if}

{#if hostKeys.length}
  {@const hk = hostKeys[0]}
  <Modal title={hk.status === "changed" ? "⚠ La clé du serveur a CHANGÉ" : "Serveur inconnu"} onclose={() => answerHostKey(false)} width="560px">
    {#if hk.status === "changed"}
      <p class="danger">
        La clé d'identification de <b>{hk.host}:{hk.port}</b> ne correspond pas à celle enregistrée.
        Cela peut indiquer une attaque (interception) — ou une réinstallation du serveur.
        N'accepte que si tu sais pourquoi la clé a changé.
      </p>
    {:else}
      <p>Première connexion à <b>{hk.host}:{hk.port}</b>. Vérifie que l'empreinte correspond à celle du serveur :</p>
    {/if}
    <p class="fp"><span class="muted">{hk.algorithm}</span><br />{hk.fingerprint}</p>
    {#snippet actions()}
      <button onclick={() => answerHostKey(false)}>Refuser</button>
      <button class={hk.status === "changed" ? "danger-btn" : "primary"} onclick={() => answerHostKey(true)}>
        {hk.status === "changed" ? "Accepter la nouvelle clé" : "Faire confiance"}
      </button>
    {/snippet}
  </Modal>
{/if}

{#if textAsk}
  <Modal title={textAsk.title} onclose={() => closeAsk(false)}>
    <form onsubmit={(e) => { e.preventDefault(); closeAsk(true); }}>
      <!-- svelte-ignore a11y_autofocus -->
      <input class="full" type={textAsk.secret ? "password" : "text"} bind:value={textAsk.value} autofocus />
      {#if textAsk.check}
        <label class="inline"><input type="checkbox" bind:checked={textAsk.checked} /> {textAsk.check}</label>
      {/if}
    </form>
    {#snippet actions()}
      <button onclick={() => closeAsk(false)}>Annuler</button>
      <button class="primary" onclick={() => closeAsk(true)}>OK</button>
    {/snippet}
  </Modal>
{/if}

{#if confirmAsk}
  <Modal title="Confirmation" onclose={() => closeConfirm(false)}>
    <p>{confirmAsk.message}</p>
    {#snippet actions()}
      <button onclick={() => closeConfirm(false)}>Annuler</button>
      <button class="danger-btn" onclick={() => closeConfirm(true)}>Confirmer</button>
    {/snippet}
  </Modal>
{/if}

{#if alertText}
  <Modal title="Erreur" onclose={() => (alertText = "")}>
    <p>{alertText}</p>
    {#snippet actions()}
      <button class="primary" onclick={() => (alertText = "")}>OK</button>
    {/snippet}
  </Modal>
{/if}

<style>
  :global(:root) {
    --bg: #1b1d23;
    --panel: #21252b;
    --border: #333842;
    --hover: #2c313a;
    --selected: #2f3b52;
    --fg: #d7dae0;
    --muted: #8a919e;
    --accent: #61afef;
    --danger: #e06c75;
    --ok: #98c379;
    --mono: "Cascadia Mono", Consolas, "DejaVu Sans Mono", monospace;
    color-scheme: dark;
  }
  :global(html, body) {
    margin: 0;
    height: 100%;
    background: var(--bg);
    color: var(--fg);
    font: 13px/1.4 "Segoe UI", system-ui, "Noto Sans", sans-serif;
    overflow: hidden;
  }
  :global(button) {
    font: inherit;
    color: var(--fg);
    background: var(--hover);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 4px 10px;
    cursor: pointer;
  }
  :global(button:hover) {
    border-color: var(--accent);
  }
  :global(button.primary) {
    background: #2b5d8a;
    border-color: #3a76ad;
  }
  :global(button.danger-btn) {
    background: #7a2e35;
    border-color: #a13d47;
  }
  :global(input, select) {
    font: inherit;
    color: var(--fg);
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 4px 6px;
    min-width: 0;
  }
  :global(input:focus, select:focus) {
    outline: 1px solid var(--accent);
  }
  :global(::-webkit-scrollbar) {
    width: 10px;
    height: 10px;
  }
  :global(::-webkit-scrollbar-track) {
    background: transparent;
  }
  :global(::-webkit-scrollbar-thumb) {
    background: #3a3f4b;
    border-radius: 5px;
    border: 2px solid transparent;
    background-clip: padding-box;
  }
  :global(::-webkit-scrollbar-button) {
    display: none;
  }
  :global(.hide) {
    display: none !important;
  }
  :global(.muted) {
    color: var(--muted);
  }

  .app {
    display: grid;
    grid-template-columns: var(--sidebar) 4px 1fr;
    height: 100vh;
  }
  aside {
    display: flex;
    flex-direction: column;
    min-height: 0;
    overflow: hidden;
    background: var(--panel);
    min-width: 0;
  }
  .sidetabs {
    display: flex;
    border-bottom: 1px solid var(--border);
  }
  .sidetabs button {
    flex: 1;
    border: none;
    border-radius: 0;
    background: none;
    padding: 8px;
    color: var(--muted);
  }
  .sidetabs button.on {
    color: var(--fg);
    box-shadow: inset 0 -2px 0 var(--accent);
  }
  .sidebody {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .fill {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .sessbar {
    display: flex;
    gap: 6px;
    padding: 8px;
  }
  .search {
    margin: 0 8px 8px;
  }
  .scroll {
    flex: 1;
    overflow: auto;
    padding: 0 4px 8px;
  }
  .pad {
    padding: 16px;
  }
  .splitter {
    cursor: col-resize;
    background: var(--border);
  }
  main {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .tabbar {
    display: flex;
    background: var(--panel);
    border-bottom: 1px solid var(--border);
    overflow-x: auto;
    min-height: 34px;
  }
  .tab {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 6px 0 12px;
    border-right: 1px solid var(--border);
    cursor: pointer;
    white-space: nowrap;
    color: var(--muted);
  }
  .tab.on {
    background: var(--bg);
    color: var(--fg);
  }
  .tname {
    max-width: 180px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .x {
    border: none;
    background: none;
    padding: 0 4px;
    font-size: 11px;
    color: var(--muted);
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--muted);
  }
  .dot.open {
    background: var(--ok);
  }
  .dot.connecting {
    background: #e5c07b;
  }
  .dot.error,
  .dot.closed {
    background: var(--danger);
  }
  .terms {
    position: relative;
    flex: 1;
    min-height: 0;
  }
  .termwrap {
    position: absolute;
    inset: 0;
  }
  .overlay {
    position: absolute;
    right: 16px;
    bottom: 16px;
    display: flex;
    gap: 8px;
  }
  .welcome {
    padding: 48px;
    max-width: 640px;
  }
  .welcome h1 {
    font-weight: 300;
    font-size: 32px;
    margin: 0 0 12px;
  }
  .fp {
    font-family: var(--mono);
    background: var(--bg);
    padding: 10px;
    border-radius: 4px;
    word-break: break-all;
  }
  .danger {
    color: var(--danger);
  }
  .full {
    width: 100%;
    box-sizing: border-box;
  }
  .inline {
    display: flex;
    gap: 6px;
    align-items: center;
    margin-top: 10px;
  }
</style>
