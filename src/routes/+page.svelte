<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { api, describe, forwardLabel, protocol, type HostKeyPrompt } from "$lib/api";
  import { app, HOME, SPLIT_LABELS, type SplitMode, type Tab } from "$lib/app.svelte";
  import ImportDialog from "$lib/ImportDialog.svelte";
  import SettingsDialog from "$lib/SettingsDialog.svelte";
  import { startBus, onConnectionClosed } from "$lib/bus";
  import ContextMenu from "$lib/ContextMenu.svelte";
  import HomeTab from "$lib/HomeTab.svelte";
  import Icon from "$lib/Icon.svelte";
  import Logo from "$lib/Logo.svelte";
  import MenuBar, { type Menu, type MenuItem } from "$lib/MenuBar.svelte";
  import Modal from "$lib/Modal.svelte";
  import SessionDialog from "$lib/SessionDialog.svelte";
  import Sidebar from "$lib/Sidebar.svelte";
  import Terminal, { terminals } from "$lib/Terminal.svelte";
  import Toolbar, { type ToolButton } from "$lib/Toolbar.svelte";
  import TunnelManager from "$lib/TunnelManager.svelte";

  import pkg from "../../package.json";
  const VERSION = pkg.version;
  let about = $state(false);
  let shortcuts = $state(false);

  const term = () => (app.active ? terminals.get(app.active.key) : undefined);
  const SOON = " (prochaine étape)";

  async function toggleFullscreen() {
    const w = getCurrentWindow();
    await w.setFullscreen(!(await w.isFullscreen()));
  }
  function togglePref(k: "sidebar" | "toolbar" | "menubar" | "statusbar") {
    app.prefs[k] = !app.prefs[k];
    app.savePrefs();
  }

  function sessionItems(): MenuItem[] {
    const list = [...app.store.sessions].sort((a, b) => a.name.localeCompare(b.name));
    if (!list.length) return [{ label: "(aucune session enregistrée)", disabled: true }];
    return list.slice(0, 40).map((s) => ({ label: s.name, icon: protocol(s.kind).icon, action: () => app.openSession(s) }));
  }

  function splitItems(): MenuItem[] {
    return (Object.keys(SPLIT_LABELS) as SplitMode[]).map((m) => ({
      label: SPLIT_LABELS[m],
      icon: m === "single" ? "terminal" : "split",
      checked: app.split === m,
      action: () => app.setSplit(m),
    }));
  }

  function macroItems(): MenuItem[] {
    const list: MenuItem[] = app.store.macros.map((m) => ({
      label: `Rejouer : ${m.name}`,
      icon: "play",
      disabled: app.playing,
      action: () => app.playMacro(m),
    }));
    return [
      app.recording
        ? { label: "Arrêter l'enregistrement", icon: "stop", action: () => app.stopRecording() }
        : { label: "Enregistrer une macro", icon: "record", disabled: !app.active, action: () => app.startRecording() },
      { label: "Gérer les macros", icon: "macro", action: () => openSide("macros") },
      ...(list.length ? [{ sep: true } as MenuItem, ...list] : []),
    ];
  }

  function openSide(t: "sessions" | "tools" | "macros") {
    app.sideTab = t;
    app.prefs.sidebar = true;
  }

  function viewItems(): MenuItem[] {
    return [
      ...splitItems(),
      { sep: true },
      { label: "Barre latérale", checked: app.prefs.sidebar, shortcut: "Ctrl+Maj+B", action: () => togglePref("sidebar") },
      { label: "Barre d'outils", checked: app.prefs.toolbar, action: () => togglePref("toolbar") },
      { label: "Barre de menus", checked: app.prefs.menubar, action: () => togglePref("menubar") },
      { label: "Barre d'état", checked: app.prefs.statusbar, action: () => togglePref("statusbar") },
      { sep: true },
      { label: "Plein écran", icon: "view", shortcut: "F11", action: toggleFullscreen },
      { sep: true },
      { label: "Agrandir le texte", shortcut: "Ctrl+Maj+=", disabled: !app.active, action: () => term()?.zoom(1) },
      { label: "Réduire le texte", shortcut: "Ctrl+Maj+-", disabled: !app.active, action: () => term()?.zoom(-1) },
      { label: "Taille normale", shortcut: "Ctrl+Maj+0", disabled: !app.active, action: () => term()?.zoom(0) },
    ];
  }

  const menus: Menu[] = [
    {
      label: "Terminal",
      items: () => [
        { label: "Ouvrir un nouveau terminal local", icon: "terminal", shortcut: "Ctrl+Maj+T", action: () => app.startLocalTerminal() },
        ...app.shells.slice(1).map((sh) => ({
          label: `Nouveau terminal : ${sh.label}`,
          icon: sh.id.startsWith("wsl:") ? "wsl" : "shell",
          action: () => app.startLocalTerminal(sh.id),
        })),
        { sep: true },
        { label: "Copier", icon: "copy", shortcut: "Ctrl+Maj+C", disabled: !app.active, action: () => term()?.copy() },
        { label: "Coller", shortcut: "Ctrl+Maj+V", disabled: !app.active, action: () => term()?.paste() },
        { label: "Tout sélectionner", disabled: !app.active, action: () => term()?.selectAll() },
        { label: "Effacer l'historique", disabled: !app.active, action: () => term()?.clear() },
        { sep: true },
        { label: "MultiExec (saisie dans tous les terminaux)", icon: "multiexec", checked: app.multiExec, disabled: !app.tabs.length, action: () => app.toggleMultiExec() },
        { sep: true },
        { label: "Renommer l'onglet", icon: "edit", disabled: !app.active, action: () => app.active && app.renameTab(app.active) },
        { label: "Dupliquer l'onglet", icon: "copy", disabled: !app.active, action: () => app.active && app.duplicateTab(app.active) },
        { label: "Reconnecter", icon: "refresh", disabled: !app.active, action: () => app.active && app.reconnect(app.active) },
        { label: "Fermer l'onglet", icon: "close", shortcut: "Ctrl+Maj+W", disabled: !app.active, action: () => app.active && app.closeTab(app.active) },
        { sep: true },
        { label: "Quitter", icon: "exit", action: quit },
      ],
    },
    {
      label: "Sessions",
      items: () => [
        { label: "Nouvelle session", icon: "session", shortcut: "Ctrl+Maj+N", action: () => app.newSession() },
        { label: "Nouveau dossier", icon: "newfolder", action: () => app.newFolder("") },
        { label: "Connexion rapide…", icon: "search", action: focusQuick },
        { sep: true },
        { label: "Importer des sessions (MobaXterm, PuTTY, OpenSSH)…", icon: "download", action: () => (app.showImport = true) },
        { sep: true },
        ...sessionItems(),
      ],
    },
    { label: "Affichage", items: viewItems },
    {
      label: "Serveur X",
      items: () => [
        { label: "Démarrer le serveur X", icon: "xserver", disabled: !!app.xserver?.running, action: () => app.toggleX() },
        { label: "Arrêter le serveur X", icon: "xserver_off", disabled: !(app.xserver?.running && app.xserver.owned), action: () => app.toggleX() },
        { sep: true },
        { label: xInfo(), disabled: true },
      ],
    },
    {
      label: "Outils",
      items: () => [
        ...app.shells.map((sh) => ({
          label: sh.label,
          icon: sh.id.startsWith("wsl:") ? "wsl" : "shell",
          action: () => app.startLocalTerminal(sh.id),
        })),
        { sep: true },
        { label: "Scanner de ports" + SOON, icon: "scan", disabled: true },
        { label: "Ports ouverts / connexions" + SOON, icon: "ports", disabled: true },
        { label: "Wake-on-LAN" + SOON, icon: "wol", disabled: true },
        { label: "Gestionnaire de tunnels SSH", icon: "tunnel", action: openTunnels },
        { sep: true },
        { label: "Serveurs intégrés (HTTP, TFTP, FTP, SSH)" + SOON, icon: "servers", disabled: true },
      ],
    },
    {
      label: "Paramètres",
      items: () => [
        { label: "Configuration…", icon: "settings", action: () => (app.showSettings = true) },
        { label: "Importer des sessions…", icon: "download", action: () => (app.showImport = true) },
        { sep: true },
        { label: "Afficher la barre latérale", checked: app.prefs.sidebar, action: () => togglePref("sidebar") },
      ],
    },
    { label: "Macros", items: macroItems },
    {
      label: "Aide",
      items: () => [
        { label: "Raccourcis clavier", icon: "keyboard", action: () => (shortcuts = true) },
        { sep: true },
        { label: "À propos d'Orbiterm", icon: "info", action: () => (about = true) },
      ],
    },
  ];

  function dropMenu(ev: MouseEvent, items: MenuItem[]) {
    const r = (ev.currentTarget as HTMLElement).getBoundingClientRect();
    app.ctx = { x: r.left, y: r.bottom, items };
  }

  const toolsLeft: ToolButton[] = $derived([
    { id: "session", label: "Session", icon: "session", title: "Nouvelle session (Ctrl+Maj+N)", action: () => app.newSession() },
    {
      id: "servers",
      label: "Serveurs",
      icon: "servers",
      title: "Serveurs intégrés",
      action: () => {
        app.sideTab = "tools";
        app.prefs.sidebar = true;
      },
    },
    {
      id: "tools",
      label: "Outils",
      icon: "tools",
      action: () => {
        app.sideTab = "tools";
        app.prefs.sidebar = true;
      },
    },
    { id: "sessions", label: "Sessions", icon: "sessions", title: "Sessions enregistrées", action: (e: MouseEvent) => dropMenu(e, sessionItems()) },
    { id: "view", label: "Affichage", icon: "view", action: (e: MouseEvent) => dropMenu(e, viewItems()) },
    { id: "split", label: "Split", icon: "split", title: "Écran partagé", active: app.split !== "single", action: (e: MouseEvent) => dropMenu(e, splitItems()) },
    {
      id: "multiexec",
      label: "MultiExec",
      icon: "multiexec",
      title: "Saisie simultanée dans tous les terminaux",
      active: app.multiExec,
      disabled: !app.tabs.length && !app.multiExec,
      action: () => app.toggleMultiExec(),
    },
    { id: "tunnel", label: "Tunnels", icon: "tunnel", title: "Gestionnaire de tunnels SSH", action: openTunnels },
    { id: "packages", label: "Paquets", icon: "packages", title: "Gestion de paquets" + SOON, disabled: true, action: () => {} },
    { id: "settings", label: "Paramètres", icon: "settings", title: "Configuration", action: () => (app.showSettings = true) },
    { id: "help", label: "Aide", icon: "help", action: () => (about = true) },
  ]);
  const toolsRight: ToolButton[] = $derived([
    {
      id: "xserver",
      label: "Serveur X",
      icon: app.xserver?.running ? "xserver" : "xserver_off",
      title: xInfo() + (app.xserver?.running ? "" : " — cliquer pour démarrer"),
      active: !!app.xserver?.running,
      action: () => app.toggleX(),
    },
    { id: "exit", label: "Quitter", icon: "exit", action: () => quit() },
  ]);

  function xInfo() {
    const x = app.xserver;
    if (!x || !x.kind) return x?.running ? "Serveur X actif (127.0.0.1:0)" : "Aucun serveur X installé (VcXsrv recommandé)";
    if (x.kind === "native") return `Serveur X du système (${x.display})`;
    return `${x.kind === "vcxsrv" ? "VcXsrv" : "Xming"} : ${x.running ? "actif sur " + x.display : "arrêté"}`;
  }

  function openTunnels() {
    app.refreshTunnels();
    app.showTunnels = true;
  }

  async function showForwards(t: Tab) {
    if (!t.connId) return;
    const list = await api.termForwards(t.connId).catch(() => []);
    app.info(list.length ? list.map((f) => forwardLabel(f.forward, f.port)).join("\n") : "Aucune redirection de port sur cette session.");
  }

  async function quit() {
    const open = app.tabs.filter((t) => t.state === "open").length;
    if (open && app.prefs.confirmQuit && !(await app.confirm(`${open} session(s) ouverte(s). Quitter quand même ?`))) return;
    await app.closeAllTabs();
    await getCurrentWindow().close();
  }

  function focusQuick() {
    app.sideTab = "sessions";
    app.prefs.sidebar = true;
    requestAnimationFrame(() => document.querySelector<HTMLInputElement>("[data-quick]")?.focus());
  }

  function tabMenu(e: MouseEvent, t: Tab) {
    e.preventDefault();
    app.activeKey = t.key;
    app.ctx = {
      x: e.clientX,
      y: e.clientY,
      items: [
        { label: "Renommer l'onglet", icon: "edit", action: () => app.renameTab(t) },
        { label: "Dupliquer l'onglet", icon: "copy", action: () => app.duplicateTab(t) },
        { label: "Reconnecter", icon: "refresh", action: () => app.reconnect(t) },
        {
          label: "Redirections de ports actives…",
          icon: "tunnel",
          disabled: t.session.kind !== "ssh" || t.state !== "open",
          action: () => showForwards(t),
        },
        { sep: true },
        { label: "Fermer l'onglet", icon: "close", action: () => app.closeTab(t) },
        {
          label: "Fermer les autres onglets",
          disabled: app.tabs.length < 2,
          action: async () => {
            for (const o of app.tabs.filter((x) => x.key !== t.key)) await app.closeTab(o);
          },
        },
        { label: "Fermer tous les onglets", action: () => app.closeAllTabs() },
      ],
    };
  }

  // ------------------------------------------------------------ sidebar resize
  function startResize(e: PointerEvent) {
    const startX = e.clientX;
    const startW = app.prefs.sidebarWidth;
    const move = (ev: PointerEvent) => (app.prefs.sidebarWidth = Math.min(600, Math.max(180, startW + ev.clientX - startX)));
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      app.savePrefs();
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  function keys(e: KeyboardEvent) {
    const cs = e.ctrlKey && e.shiftKey;
    if (cs && e.code === "KeyW" && app.active) {
      e.preventDefault();
      app.closeTab(app.active);
    } else if (cs && e.code === "KeyT") {
      e.preventDefault();
      app.startLocalTerminal();
    } else if (cs && e.code === "KeyN") {
      e.preventDefault();
      app.newSession();
    } else if (cs && e.code === "KeyB") {
      e.preventDefault();
      togglePref("sidebar");
    } else if (cs && (e.code === "Equal" || e.code === "NumpadAdd")) {
      e.preventDefault();
      term()?.zoom(1);
    } else if (cs && (e.code === "Minus" || e.code === "NumpadSubtract")) {
      e.preventDefault();
      term()?.zoom(-1);
    } else if (cs && (e.code === "Digit0" || e.code === "Numpad0")) {
      e.preventDefault();
      term()?.zoom(0);
    } else if (e.ctrlKey && e.code === "Tab") {
      e.preventDefault();
      app.nextTab(e.shiftKey ? -1 : 1);
    } else if (e.code === "F11") {
      e.preventDefault();
      toggleFullscreen();
    }
  }

  // ------------------------------------------------------------ split screen
  const splitOn = $derived(app.split !== "single");

  function paneRect(i: number) {
    const n = Math.max(1, app.slots);
    const cols = app.split === "v2" || app.split === "grid4" ? 2 : app.split === "h2" ? 1 : Math.ceil(Math.sqrt(n));
    const rows = Math.max(1, Math.ceil(n / cols));
    const c = i % cols;
    const r = Math.floor(i / cols);
    return `left:${(c * 100) / cols}%;top:${(r * 100) / rows}%;width:${100 / cols}%;height:${100 / rows}%`;
  }

  $effect(() => {
    // re-run on tab list / active tab / layout changes
    void app.tabs.map((t) => t.key).join();
    void app.activeKey;
    void app.split;
    untrack(() => app.syncPanes());
  });

  const stateLabel: Record<string, string> = { connecting: "connexion…", open: "connecté", closed: "terminé", error: "erreur" };

  onMount(() => {
    const unsubs: Array<() => void> = [];
    startBus();
    app.init();
    listen<HostKeyPrompt>("hostkey-prompt", (e) => app.hostKeys.push(e.payload)).then((u) => unsubs.push(u));
    unsubs.push(onConnectionClosed((connId) => app.onConnectionClosed(connId)));
    // X server and standalone tunnels can change outside the app
    const poll = setInterval(() => {
      app.refreshX();
      if (app.tunnels.some((t) => t.running) || app.showTunnels) app.refreshTunnels();
    }, 5000);
    unsubs.push(() => clearInterval(poll));
    return () => unsubs.forEach((u) => u());
  });
</script>

<svelte:window onkeydown={keys} />

<div class="app">
  {#if app.prefs.menubar}<MenuBar {menus} />{/if}
  {#if app.prefs.toolbar}<Toolbar left={toolsLeft} right={toolsRight} />{/if}

  <div class="work">
    <Sidebar />
    {#if app.prefs.sidebar}
      <div class="splitter" role="separator" aria-orientation="vertical" onpointerdown={startResize}></div>
    {/if}

    <main>
      <div class="tabbar" role="tablist">
        <button
          class="tab home"
          class:on={app.activeKey === HOME}
          role="tab"
          aria-selected={app.activeKey === HOME}
          title="Accueil"
          data-tab="home"
          onclick={() => (app.activeKey = HOME)}
        >
          <Icon name="home" size={16} />
        </button>
        {#each app.tabs as t, i (t.key)}
          <div
            class="tab"
            class:on={t.key === app.activeKey}
            role="tab"
            tabindex="0"
            aria-selected={t.key === app.activeKey}
            title={describe(t.session)}
            data-tab={t.title}
            onclick={() => (app.activeKey = t.key)}
            onkeydown={(e) => e.key === "Enter" && (app.activeKey = t.key)}
            onauxclick={(e) => e.button === 1 && app.closeTab(t)}
            oncontextmenu={(e) => tabMenu(e, t)}
          >
            <span class="dot {t.state}"></span>
            <Icon name={protocol(t.session.kind).icon} size={15} />
            <span class="tname">{i + 1}. {t.title}</span>
            <button
              class="x"
              title="Fermer (Ctrl+Maj+W)"
              aria-label="Fermer l'onglet"
              onclick={(e) => {
                e.stopPropagation();
                app.closeTab(t);
              }}><Icon name="close" size={11} /></button
            >
          </div>
        {/each}
        <button class="tab plus" title="Nouveau terminal local (Ctrl+Maj+T)" aria-label="Nouveau terminal local" onclick={() => app.startLocalTerminal()}>
          <Icon name="plus" size={14} />
        </button>
      </div>

      {#if app.multiExec}
        <div class="mebar" data-multiexec>
          <Icon name="multiexec" size={16} />
          <span>
            <b>MultiExec</b> — ce que tu tapes est envoyé à {app.targets().length} terminal(aux). Décoche un terminal pour l'exclure.
          </span>
          <button onclick={() => app.toggleMultiExec()}>Quitter MultiExec</button>
        </div>
      {/if}

      <div class="terms" class:split={splitOn}>
        <div class="termwrap" class:hide={app.activeKey !== HOME}><HomeTab /></div>
        {#if splitOn && app.activeKey !== HOME}
          {#each { length: Math.max(0, app.slots - app.panes.length) } as _, j (j)}
            <div class="termwrap pane empty" style={paneRect(app.panes.length + j)}>
              <span>Sélectionne un onglet pour l'afficher ici</span>
            </div>
          {/each}
        {/if}
        {#each app.tabs as t (t.key)}
          {@const pi = splitOn ? app.panes.indexOf(t.key) : -1}
          <div
            class="termwrap"
            class:pane={splitOn}
            class:focus={splitOn && t.key === app.activeKey}
            class:hide={splitOn ? pi < 0 || app.activeKey === HOME : t.key !== app.activeKey}
            style={pi >= 0 ? paneRect(pi) : ""}
            data-conn={t.connId ?? ""}
            data-pane={pi >= 0 ? pi : null}
            onpointerdowncapture={() => splitOn && (app.activeKey = t.key)}
          >
            {#if splitOn}
              <div class="phead">
                <span class="dot {t.state}"></span>
                <Icon name={protocol(t.session.kind).icon} size={13} />
                <span class="ptitle">{t.title}</span>
                {#if app.multiExec}
                  <label class="mechk" title="Recevoir la saisie MultiExec">
                    <input type="checkbox" checked={app.meIncluded(t.key)} onchange={() => app.meToggle(t.key)} /> saisie
                  </label>
                {/if}
              </div>
            {/if}
            <div class="tbody" class:withhead={splitOn}>
              <Terminal
                tabKey={t.key}
                connId={t.connId}
                settings={t.session.terminal}
                active={t.key === app.activeKey}
                status={t.status}
                prefs={app.prefs}
                oninput={(d) => app.onInput(t.key, d)}
              />
              {#if t.state === "closed" || t.state === "error"}
                <div class="overlay">
                  <button class="primary" onclick={() => app.reconnect(t)}><Icon name="refresh" size={14} /> Redémarrer la session</button>
                  <button onclick={() => app.closeTab(t)}>Fermer l'onglet</button>
                </div>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    </main>
  </div>

  {#if app.prefs.statusbar}
    <footer class="status">
      <span class="cell"><Logo size={14} /> Orbiterm {VERSION}</span>
      <span class="cell grow">
        {#if app.active}
          <Icon name={protocol(app.active.session.kind).icon} size={13} />
          {describe(app.active.session)} — {stateLabel[app.active.state]}
        {:else}
          Accueil
        {/if}
      </span>
      {#if app.recording}
        <button class="cell link rec" title="Arrêter l'enregistrement de la macro" onclick={() => app.stopRecording()}>
          ● Enregistrement macro ({app.recording.steps.length})
        </button>
      {/if}
      {#if app.multiExec}<span class="cell"><Icon name="multiexec" size={13} /> MultiExec</span>{/if}
      <span class="cell">{app.tabs.length} onglet(s) · {app.store.sessions.length} session(s) enregistrée(s)</span>
      {#if app.tunnels.some((t) => t.running)}
        <button class="cell link" title="Gestionnaire de tunnels" onclick={openTunnels}>
          <Icon name="tunnel" size={13} /> {app.tunnels.filter((t) => t.running).length} tunnel(s) actif(s)
        </button>
      {/if}
      <button class="cell link" title={xInfo()} onclick={() => app.toggleX()}>
        <Icon name={app.xserver?.running ? "xserver" : "xserver_off"} size={13} /> X {app.xserver?.running ? "actif" : "arrêté"}
      </button>
    </footer>
  {/if}
</div>

<ContextMenu />

{#if app.editing}
  <SessionDialog session={app.editing} />
{/if}

{#if app.showTunnels}
  <TunnelManager />
{/if}

{#if app.showSettings}
  <SettingsDialog />
{/if}

{#if app.showImport}
  <ImportDialog />
{/if}

{#if app.hostKeys.length}
  {@const hk = app.hostKeys[0]}
  <Modal title={hk.status === "changed" ? "⚠ La clé du serveur a CHANGÉ" : "Serveur inconnu"} onclose={() => app.answerHostKey(false)} width="560px">
    <div class="msg">
      <Icon name={hk.status === "changed" ? "warning" : "info"} size={32} />
      <div>
        {#if hk.status === "changed"}
          <p class="danger">
            La clé d'identification de <b>{hk.host}:{hk.port}</b> ne correspond pas à celle enregistrée. Cela peut
            indiquer une attaque (interception) — ou une réinstallation du serveur. N'accepte que si tu sais pourquoi la clé
            a changé.
          </p>
        {:else}
          <p>Première connexion à <b>{hk.host}:{hk.port}</b>. Vérifie que l'empreinte correspond à celle du serveur :</p>
        {/if}
        <p class="fp"><span class="muted">{hk.algorithm}</span><br />{hk.fingerprint}</p>
      </div>
    </div>
    {#snippet actions()}
      <button class={hk.status === "changed" ? "danger-btn" : "primary"} onclick={() => app.answerHostKey(true)}>
        {hk.status === "changed" ? "Accepter la nouvelle clé" : "Faire confiance"}
      </button>
      <button onclick={() => app.answerHostKey(false)}>Refuser</button>
    {/snippet}
  </Modal>
{/if}

{#if app.textAsk}
  <Modal title={app.textAsk.title} onclose={() => app.closeAsk(false)}>
    <form
      onsubmit={(e) => {
        e.preventDefault();
        app.closeAsk(true);
      }}
    >
      <!-- svelte-ignore a11y_autofocus -->
      <input class="full" type={app.textAsk.secret ? "password" : "text"} bind:value={app.textAsk.value} autofocus />
      {#if app.textAsk.check}
        <label class="inline"><input type="checkbox" bind:checked={app.textAsk.checked} /> {app.textAsk.check}</label>
      {/if}
    </form>
    {#snippet actions()}
      <button class="primary dlg" onclick={() => app.closeAsk(true)}>OK</button>
      <button class="dlg" onclick={() => app.closeAsk(false)}>Annuler</button>
    {/snippet}
  </Modal>
{/if}

{#if app.confirmAsk}
  <Modal title="Confirmation" onclose={() => app.closeConfirm(false)}>
    <div class="msg"><Icon name="warning" size={32} /><p>{app.confirmAsk.message}</p></div>
    {#snippet actions()}
      <button class="danger-btn dlg" onclick={() => app.closeConfirm(true)}>Oui</button>
      <button class="dlg" onclick={() => app.closeConfirm(false)}>Non</button>
    {/snippet}
  </Modal>
{/if}

{#if app.alertText}
  <Modal title="Erreur" onclose={() => (app.alertText = "")}>
    <div class="msg"><Icon name="warning" size={32} /><p>{app.alertText}</p></div>
    {#snippet actions()}
      <button class="primary dlg" onclick={() => (app.alertText = "")}>OK</button>
    {/snippet}
  </Modal>
{/if}

{#if app.infoText}
  <Modal title="Information" onclose={() => (app.infoText = "")}>
    <div class="msg"><Icon name="info" size={32} /><p>{app.infoText}</p></div>
    {#snippet actions()}
      <button class="primary dlg" onclick={() => (app.infoText = "")}>OK</button>
    {/snippet}
  </Modal>
{/if}

{#if about}
  <Modal title="À propos d'Orbiterm" onclose={() => (about = false)} width="440px">
    <div class="msg">
      <Logo size={56} />
      <div>
        <p><b>Orbiterm {VERSION}</b></p>
        <p>Terminal multi-protocoles pour Windows et Linux : SSH, SFTP, Telnet, Rlogin, port série, Mosh, shells locaux et WSL.</p>
        <p class="muted">Tauri 2 · Rust · Svelte · xterm.js</p>
      </div>
    </div>
    {#snippet actions()}
      <button class="primary dlg" onclick={() => (about = false)}>OK</button>
    {/snippet}
  </Modal>
{/if}

{#if shortcuts}
  <Modal title="Raccourcis clavier" onclose={() => (shortcuts = false)} width="480px">
    <table class="keys">
      <tbody>
        <tr><td>Ctrl+Maj+T</td><td>Nouveau terminal local</td></tr>
        <tr><td>Ctrl+Maj+N</td><td>Nouvelle session</td></tr>
        <tr><td>Ctrl+Maj+W</td><td>Fermer l'onglet</td></tr>
        <tr><td>Ctrl+Tab / Ctrl+Maj+Tab</td><td>Onglet suivant / précédent</td></tr>
        <tr><td>Ctrl+Maj+C / Ctrl+Maj+V</td><td>Copier / coller</td></tr>
        <tr><td>Sélection / clic droit</td><td>Copie automatique / coller</td></tr>
        <tr><td>Maj+Inser</td><td>Coller</td></tr>
        <tr><td>Ctrl+Maj+= / - / 0</td><td>Taille du texte</td></tr>
        <tr><td>Ctrl+Maj+B</td><td>Afficher / masquer la barre latérale</td></tr>
        <tr><td>F11</td><td>Plein écran</td></tr>
      </tbody>
    </table>
    {#snippet actions()}
      <button class="primary dlg" onclick={() => (shortcuts = false)}>OK</button>
    {/snippet}
  </Modal>
{/if}

<style>
  :global(:root) {
    --chrome: #f0f0f0;
    --chrome-border: #cfcfcf;
    --chrome-border-dark: #a0a0a0;
    --panel: #ffffff;
    --hover: #e5f1fb;
    --hover-border: #a8cdef;
    --selected: #cce4f7;
    --text: #1b1b1b;
    --text-disabled: #a0a0a0;
    --muted: #6b6b6b;
    --accent: #0a64c2;
    --brand: #1b4f8f;
    --danger: #c42b1c;
    --ok: #2e9d4a;
    --input-bg: #ffffff;
    --toolbar-top: #fbfbfb;
    --toolbar-bottom: #e8ebee;
    --titlebar: #ffffff;
    --titlebar-text: #1b1b1b;
    --home-bg: #e4e8ec;
    --mono: "Cascadia Mono", Consolas, "DejaVu Sans Mono", monospace;
    /* aliases used by older components */
    --fg: var(--text);
    --bg: var(--panel);
    --border: var(--chrome-border);
    color-scheme: light;
  }
  :global(html, body) {
    margin: 0;
    height: 100%;
    background: var(--chrome);
    color: var(--text);
    font: 12.5px/1.4 "Segoe UI", system-ui, "Noto Sans", sans-serif;
    overflow: hidden;
  }
  :global(button) {
    font: inherit;
    color: var(--text);
    background: #fdfdfd;
    border: 1px solid #adadad;
    border-radius: 2px;
    padding: 3px 10px;
    cursor: default;
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
  :global(button:hover:not(:disabled)) {
    background: var(--hover);
    border-color: #0078d7;
  }
  :global(button:disabled) {
    color: var(--text-disabled);
  }
  :global(button.primary) {
    border-color: #0078d7;
    box-shadow: inset 0 0 0 1px #0078d7;
  }
  :global(button.danger-btn) {
    border-color: var(--danger);
    color: var(--danger);
  }
  :global(input, select) {
    font: inherit;
    color: var(--text);
    background: var(--input-bg);
    border: 1px solid #a0a0a0;
    border-radius: 0;
    padding: 3px 5px;
    min-width: 0;
  }
  :global(input:disabled, select:disabled) {
    background: var(--chrome);
    color: var(--text-disabled);
  }
  :global(input:focus, select:focus) {
    outline: none;
    border-color: #0078d7;
  }
  :global(::-webkit-scrollbar) {
    width: 12px;
    height: 12px;
  }
  :global(::-webkit-scrollbar-track) {
    background: #f0f0f0;
  }
  :global(::-webkit-scrollbar-thumb) {
    background: #c1c1c1;
    border: 3px solid #f0f0f0;
  }
  :global(::-webkit-scrollbar-thumb:hover) {
    background: #a8a8a8;
  }
  :global(.hide) {
    display: none !important;
  }
  :global(.muted) {
    color: var(--muted);
  }

  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }
  .work {
    flex: 1;
    display: flex;
    min-height: 0;
  }
  .splitter {
    width: 4px;
    cursor: col-resize;
    background: var(--chrome);
    border-left: 1px solid var(--chrome-border);
    border-right: 1px solid var(--chrome-border);
  }
  main {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .tabbar {
    display: flex;
    align-items: flex-end;
    gap: 1px;
    background: var(--chrome);
    border-bottom: 1px solid var(--chrome-border-dark);
    overflow-x: auto;
    overflow-y: hidden;
    height: 27px;
    padding: 2px 2px 0;
    flex: 0 0 auto;
  }
  .tabbar::-webkit-scrollbar {
    height: 0;
  }
  .tab {
    display: flex;
    align-items: center;
    gap: 5px;
    height: 24px;
    padding: 0 4px 0 8px;
    border: 1px solid var(--chrome-border);
    border-bottom: none;
    border-radius: 3px 3px 0 0;
    background: #e3e3e3;
    cursor: default;
    white-space: nowrap;
    color: var(--text);
    box-sizing: border-box;
  }
  .tab:hover {
    background: var(--hover);
  }
  .tab.on {
    background: var(--panel);
    border-color: var(--chrome-border-dark);
    height: 25px;
    margin-bottom: -1px;
    box-shadow: inset 0 2px 0 var(--accent);
  }
  .tab.home,
  .tab.plus {
    padding: 0 8px;
  }
  .tab.plus {
    background: none;
    border-color: transparent;
  }
  .tname {
    max-width: 200px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .x {
    border: none;
    background: none;
    padding: 3px;
    color: var(--muted);
    border-radius: 2px;
  }
  .x:hover:not(:disabled) {
    background: #e81123;
    color: #fff;
    border: none;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--muted);
  }
  .dot.open {
    background: #2fbf5a;
  }
  .dot.connecting {
    background: #e5a50a;
  }
  .dot.error,
  .dot.closed {
    background: #d9363e;
  }
  .terms {
    position: relative;
    flex: 1;
    min-height: 0;
    background: #000;
  }
  .termwrap {
    position: absolute;
    inset: 0;
  }
  .tbody {
    position: absolute;
    inset: 0;
  }
  .tbody.withhead {
    top: 20px;
  }
  .termwrap.pane {
    inset: auto;
    box-sizing: border-box;
    border: 1px solid #3a3a3a;
  }
  .termwrap.pane.focus {
    border-color: var(--accent);
  }
  .termwrap.empty {
    display: flex;
    align-items: center;
    justify-content: center;
    color: #777;
    background: #111;
  }
  .phead {
    display: flex;
    align-items: center;
    gap: 5px;
    height: 20px;
    padding: 0 6px;
    background: #d9d9d9;
    color: #222;
    font-size: 11.5px;
    user-select: none;
  }
  .pane.focus .phead {
    background: #cce4f7;
  }
  .ptitle {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mechk {
    display: flex;
    align-items: center;
    gap: 3px;
  }
  .mechk input {
    margin: 0;
  }
  .mebar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 3px 8px;
    background: #fff4c2;
    border-bottom: 1px solid #e0c96a;
  }
  .mebar span {
    flex: 1;
  }
  .mebar button {
    padding: 1px 8px;
  }
  button.cell.rec {
    color: var(--danger);
  }
  .overlay {
    position: absolute;
    right: 16px;
    bottom: 16px;
    display: flex;
    gap: 8px;
  }
  .status {
    display: flex;
    height: 22px;
    background: var(--chrome);
    border-top: 1px solid var(--chrome-border);
    font-size: 11.5px;
    color: #333;
    user-select: none;
  }
  .cell {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 0 10px;
    border-right: 1px solid var(--chrome-border);
    white-space: nowrap;
  }
  .cell.grow {
    flex: 1;
    overflow: hidden;
  }
  button.cell {
    border: none;
    border-right: 1px solid var(--chrome-border);
    border-radius: 0;
    background: none;
    font-size: 11.5px;
    padding: 0 10px;
  }
  .msg p {
    white-space: pre-line;
  }
  .msg {
    display: flex;
    gap: 14px;
    align-items: flex-start;
  }
  .msg p {
    margin: 0 0 8px;
  }
  .fp {
    font-family: var(--mono);
    background: var(--chrome);
    border: 1px solid var(--chrome-border);
    padding: 8px;
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
  .dlg {
    min-width: 90px;
    justify-content: center;
  }
  .keys td {
    padding: 3px 12px 3px 0;
  }
  .keys td:first-child {
    font-family: var(--mono);
    white-space: nowrap;
  }
</style>
