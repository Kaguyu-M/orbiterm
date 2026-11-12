<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { theme } from "$lib/theme.svelte";
  import { api, describe, forwardLabel, protocol, viewOf, type HostKeyPrompt, type ServerKind } from "$lib/api";
  import BrowserView from "$lib/BrowserView.svelte";
  import EditorView from "$lib/EditorView.svelte";
  import DiffView from "$lib/DiffView.svelte";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { checkUpdate, type UpdateInfo } from "$lib/update";
  import UpdateDialog from "$lib/UpdateDialog.svelte";
  import ExternalView from "$lib/ExternalView.svelte";
  import FilesView from "$lib/FilesView.svelte";
  import VncView from "$lib/VncView.svelte";
  import RdpView from "$lib/RdpView.svelte";
  import RemoteInfo from "$lib/RemoteInfo.svelte";
  import TermBar from "$lib/TermBar.svelte";
  import { app, HOME, SPLIT_LABELS, type SplitMode, type Tab, UI_THEMES, type UiTheme } from "$lib/app.svelte";
  import ImportDialog from "$lib/ImportDialog.svelte";
  import PasswordsDialog from "$lib/PasswordsDialog.svelte";
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
  import ServersDialog from "$lib/ServersDialog.svelte";
  import PortScanner from "$lib/PortScanner.svelte";
  import OpenPorts from "$lib/OpenPorts.svelte";
  import WakeOnLan from "$lib/WakeOnLan.svelte";
  import KeyGenerator from "$lib/KeyGenerator.svelte";

  import pkg from "../../package.json";
  const VERSION = pkg.version;
  let upd: { state: "idle" | "checking" | "done" | "error"; info?: UpdateInfo; msg?: string } = $state({ state: "idle" });
  /** version compared with the publications (tests may pretend to run an older one) */
  const current = () => (window as unknown as { __orbitermVersion?: string }).__orbitermVersion ?? VERSION;
  /** new version offered in the update dialog */
  let offer = $state<UpdateInfo | null>(null);
  async function runUpdateCheck() {
    about = true;
    upd = { state: "checking" };
    try {
      upd = { state: "done", info: await checkUpdate(current()) };
      if (upd.info?.newer) {
        about = false;
        offer = upd.info;
      }
    } catch (e) {
      upd = { state: "error", msg: e instanceof Error ? e.message : String(e) };
    }
  }
  /** At start-up: silent, and only speaks when a version the user has not skipped exists. */
  async function startupUpdateCheck() {
    if (!app.prefs.updateCheck) return;
    try {
      const info = await checkUpdate(current());
      upd = { state: "done", info };
      if (info.newer && info.latest !== app.prefs.updateSkip && !offer) offer = info;
    } catch {
      // no network: nothing to say
    }
  }
  let about = $state(false);
  let shortcuts = $state(false);

  const term = () => (app.active ? terminals.get(app.active.key) : undefined);

  async function toggleFullscreen() {
    const w = getCurrentWindow();
    await w.setFullscreen(!(await w.isFullscreen()));
  }
  function toggleDark() {
    // the grey themes switch between themselves, the others between crème and brun
    const cur = app.prefs.uiTheme;
    if (cur === "graphite") app.prefs.uiTheme = "graphite-light";
    else if (cur === "graphite-light") app.prefs.uiTheme = "graphite";
    else app.prefs.uiTheme = document.documentElement.dataset.theme === "dark" ? "light" : "dark";
    app.savePrefs();
  }
  function setUiTheme(id: UiTheme) {
    app.prefs.uiTheme = id;
    app.savePrefs();
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
      { label: "Mode sombre", checked: document.documentElement.dataset.theme === "dark", action: toggleDark },
      ...UI_THEMES.map((t) => ({ label: `Thème : ${t.label}`, checked: (app.prefs.uiTheme ?? "system") === t.id, action: () => setUiTheme(t.id) })),
      { sep: true as const },
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
        { label: "Rechercher…", icon: "search", shortcut: "Ctrl+Maj+F", disabled: !app.active, action: () => term()?.search() },
        { label: "Effacer l'historique", disabled: !app.active, action: () => term()?.clear() },
        { sep: true },
        { label: "MultiExec (saisie dans tous les terminaux)", icon: "multiexec", checked: app.multiExec, disabled: !app.tabs.length, action: () => app.toggleMultiExec() },
        { sep: true },
        { label: "Renommer l'onglet", icon: "edit", disabled: !app.active, action: () => app.active && app.renameTab(app.active) },
        { label: "Dupliquer l'onglet", icon: "copy", disabled: !app.active, action: () => app.active && app.duplicateTab(app.active) },
        { label: "Reconnecter", icon: "refresh", disabled: !app.active, action: () => app.active && app.reconnect(app.active) },
        { label: "Ouvrir une session SFTP séparée", icon: "sftp", disabled: app.active?.session.kind !== "ssh", action: () => app.active && app.openSeparateSftp(app.active) },
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
        { label: "Exporter la configuration…", icon: "upload", action: () => app.exportConfig() },
        { label: "Importer une configuration Orbiterm…", icon: "download", action: () => app.importConfig() },
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
        { label: "Éditeur de texte (nouveau fichier)", icon: "edit", action: () => app.openEditor("") },
        { label: "Ouvrir un fichier dans l'éditeur…", icon: "file", shortcut: "Ctrl+Maj+O", action: openInEditor },
        { label: "Comparer deux fichiers…", icon: "split", action: () => app.openDiff("", "", "files") },
        { label: "Comparer deux dossiers…", icon: "folder", action: () => app.openDiff("", "", "dirs") },
        { label: "Générateur de clés SSH…", icon: "key", action: () => (app.netTool = "keygen") },
        { sep: true },
        ...netItems(),
        { sep: true },
        ...serverItems(),
      ],
    },
    {
      label: "Paramètres",
      items: () => [
        { label: "Configuration…", icon: "settings", action: () => (app.showSettings = true) },
        { label: "Gestion des mots de passe…", icon: "key", action: () => (app.showPasswords = true) },
        { label: "Importer des sessions…", icon: "download", action: () => (app.showImport = true) },
        { sep: true },
        { label: "Exporter la configuration…", icon: "upload", action: () => app.exportConfig() },
        { label: "Importer une configuration Orbiterm…", icon: "download", action: () => app.importConfig() },
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
        { label: "Rechercher les mises à jour…", icon: "refresh", action: runUpdateCheck },
        { label: "À propos d'Orbiterm", icon: "info", action: () => (about = true) },
      ],
    },
  ];

  async function openInEditor() {
    const picked = await openDialog({ multiple: true, directory: false, title: "Ouvrir dans l'éditeur" });
    for (const f of Array.isArray(picked) ? picked : picked ? [picked] : []) app.openEditor(f);
  }

  function netItems(): MenuItem[] {
    return [
      { label: "Scanner de ports…", icon: "scan", action: () => (app.netTool = "scan") },
      { label: "Ports ouverts / connexions…", icon: "ports", action: () => (app.netTool = "ports") },
      { label: "Wake-on-LAN…", icon: "wol", action: () => (app.netTool = "wol") },
      { label: "Gestionnaire de tunnels SSH", icon: "tunnel", action: openTunnels },
    ];
  }

  const SERVER_LABELS: Array<[ServerKind, string, string]> = [
    ["http", "Serveur HTTP", "web"],
    ["tftp", "Serveur TFTP", "network"],
    ["ftp", "Serveur FTP", "ftp"],
    ["ssh", "Serveur SSH/SFTP", "ssh"],
    ["telnet", "Serveur Telnet", "telnet"],
    ["iperf", "Serveur Iperf", "scan"],
    ["vnc", "Serveur VNC", "vnc"],
  ];
  function serverItems(): MenuItem[] {
    return [
      { label: "Serveurs intégrés…", icon: "servers", action: () => app.openServers() },
      ...SERVER_LABELS.map(([k, l, icon]) => {
        const s = app.servers.find((x) => x.config.kind === k);
        return { label: s?.running ? `${l} — actif (port ${s.port})` : l, icon, checked: !!s?.running, action: () => app.openServers(k) } as MenuItem;
      }),
    ];
  }

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
      title: "Serveurs intégrés (HTTP, TFTP, FTP, SSH)",
      active: app.servers.some((s) => s.running),
      action: () => app.openServers(),
    },
    {
      id: "tools",
      label: "Outils",
      icon: "tools",
      title: "Outils réseau",
      action: (e: MouseEvent) => dropMenu(e, [...netItems(), { sep: true }, ...app.shells.map((sh) => ({ label: sh.label, icon: sh.id.startsWith("wsl:") ? "wsl" : "shell", action: () => app.startLocalTerminal(sh.id) }))]),
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
    const unsaved = app.tabs.filter((t) => t.dirty);
    if (unsaved.length) {
      const names = unsaved.map((t) => `« ${t.title} »`).join(", ");
      if (!(await app.confirm(`Modifications non enregistrées : ${names}. Quitter sans enregistrer ?`))) return;
      for (const t of unsaved) t.dirty = false;
    }
    const open = app.tabs.filter((t) => t.state === "open" && t.session.kind !== "editor" && t.session.kind !== "diff").length;
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
        { label: "Dupliquer l'onglet", icon: "copy", disabled: t.session.kind === "editor" || t.session.kind === "diff", action: () => app.duplicateTab(t) },
        { label: "Reconnecter", icon: "refresh", disabled: t.session.kind === "editor" || t.session.kind === "diff", action: () => app.reconnect(t) },
        {
          label: "Redirections de ports actives…",
          icon: "tunnel",
          disabled: t.session.kind !== "ssh" || t.state !== "open",
          action: () => showForwards(t),
        },
        {
          label: "Ouvrir une session SFTP séparée",
          icon: "sftp",
          disabled: t.session.kind !== "ssh",
          action: () => app.openSeparateSftp(t),
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
    } else if (cs && e.code === "KeyO") {
      e.preventDefault();
      openInEditor();
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

  // thème de l'interface (clair / sombre / suivre le système)
  let systemDark = $state(typeof matchMedia !== "undefined" && matchMedia("(prefers-color-scheme: dark)").matches);
  $effect(() => {
    const mq = matchMedia("(prefers-color-scheme: dark)");
    const on = (e: MediaQueryListEvent) => (systemDark = e.matches);
    mq.addEventListener("change", on);
    return () => mq.removeEventListener("change", on);
  });
  $effect(() => {
    const pref = app.prefs.uiTheme ?? "system";
    const known = UI_THEMES.find((t) => t.id === pref) ?? UI_THEMES[0];
    const dark = known.dark ?? systemDark;
    const root = document.documentElement;
    // data-theme keeps the light / dark family; data-skin selects a palette on top of it
    root.dataset.theme = dark ? "dark" : "light";
    if (known.id === "cyber" || known.id === "graphite" || known.id === "graphite-light") root.dataset.skin = known.id;
    else delete root.dataset.skin;
    theme.dark = dark;
    getCurrentWindow().setTheme(known.dark === null ? null : dark ? "dark" : "light").catch(() => {});
  });

  const stateLabel: Record<string, string> = { connecting: "connexion…", open: "connecté", closed: "terminé", error: "erreur" };

  onMount(() => {
    const unsubs: Array<() => void> = [];
    startBus();
    app.init();
    const updTimer = setTimeout(startupUpdateCheck, 5000);
    unsubs.push(() => clearTimeout(updTimer));
    listen<HostKeyPrompt>("hostkey-prompt", (e) => app.hostKeys.push(e.payload)).then((u) => unsubs.push(u));
    unsubs.push(onConnectionClosed((connId, reason) => app.onConnectionClosed(connId, reason)));
    // X server and standalone tunnels can change outside the app
    const poll = setInterval(() => {
      app.refreshX();
      if (app.tunnels.some((t) => t.running) || app.showTunnels) app.refreshTunnels();
      if (!app.showServers) app.refreshServers();
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
            <span class="tname">{i + 1}. {t.title}{#if t.dirty}<span class="dirty" title="Modifications non enregistrées"> ●</span>{/if}</span>
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
          {@const view = viewOf(t.session)}
          {@const shown = app.activeKey !== HOME && (splitOn ? pi >= 0 : t.key === app.activeKey)}
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
              {#if view === "terminal"}
                {@const mon = app.prefs.sshMonitor && t.session.kind === "ssh" && !!t.connId && t.state === "open"}
                {@const tbar = app.prefs.termBar && protocol(t.session.kind).remote}
                {#if tbar}<div class="ttop"><TermBar tab={t} /></div>{/if}
                <div class="tmain" class:withinfo={mon} class:withbar={tbar}>
                  <Terminal
                    tabKey={t.key}
                    connId={t.connId}
                    settings={t.session.terminal}
                    active={t.key === app.activeKey}
                    status={t.status}
                    prefs={app.prefs}
                    oninput={(d) => app.onInput(t.key, d)}
                  />
                </div>
                {#if mon && t.connId}
                  <div class="tinfo"><RemoteInfo connId={t.connId} visible={shown} /></div>
                {/if}
              {:else if view === "files"}
                {#if t.connId && t.state === "open"}
                  <FilesView connId={t.connId} visible={shown} source={`${t.session.username ? t.session.username + "@" : ""}${t.session.host}`} />
                {:else if t.state === "connecting"}
                  <div class="viewmsg">{t.status || "Connexion…"}</div>
                {/if}
              {:else if view === "vnc"}
                <VncView tab={t} active={t.key === app.activeKey} />
              {:else if view === "rdp"}
                {#if t.connId}
                  <RdpView tab={t} active={t.key === app.activeKey} />
                {:else}
                  <div class="viewmsg">{t.status || "Connexion au bureau à distance…"}</div>
                {/if}
              {:else if view === "external"}
                <ExternalView tab={t} />
              {:else if view === "browser"}
                <BrowserView tab={t} visible={shown} />
              {:else if view === "editor"}
                <EditorView tab={t} visible={shown} />
              {:else if view === "diff"}
                <DiffView tab={t} />
              {/if}
              {#if (t.state === "closed" || t.state === "error") && view !== "external" && view !== "browser" && view !== "editor" && view !== "diff"}
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
          {describe(app.active.session)} — {app.active.session.kind === "editor" || app.active.session.kind === "diff" ? (app.active.dirty ? "modifié" : "enregistré") : stateLabel[app.active.state]}
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
      {#if app.servers.some((s) => s.running)}
        <button class="cell link" title="Serveurs intégrés" onclick={() => app.openServers(app.servers.find((s) => s.running)?.config.kind)} data-status-servers>
          <Icon name="servers" size={13} />
          {app.servers.filter((s) => s.running).map((s) => s.config.kind.toUpperCase() + " :" + s.port).join(" · ")}
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

{#if app.showServers}
  <ServersDialog />
{/if}

{#if app.netTool === "scan"}
  <PortScanner />
{:else if app.netTool === "ports"}
  <OpenPorts />
{:else if app.netTool === "wol"}
  <WakeOnLan />
{:else if app.netTool === "keygen"}
  <KeyGenerator />
{/if}

{#if app.showPasswords}
  <PasswordsDialog />
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
        <p class="upd" data-update>
          {#if upd.state === "checking"}
            Recherche de mises à jour…
          {:else if upd.state === "error"}
            <span class="error">Vérification impossible : {upd.msg}</span>
          {:else if upd.state === "done" && upd.info?.newer}
            <b>Version {upd.info.latest} disponible.</b>
            <button class="link" onclick={() => ((about = false), (offer = upd.info!))} data-update-open>Mettre à jour…</button>
          {:else if upd.state === "done"}
            Orbiterm est à jour.
          {:else}
            <button class="link" onclick={runUpdateCheck}>Rechercher les mises à jour</button>
          {/if}
        </p>
      </div>
    </div>
    {#snippet actions()}
      <button class="primary dlg" onclick={() => (about = false)}>OK</button>
    {/snippet}
  </Modal>
{/if}

{#if offer}
  <UpdateDialog
    info={offer}
    current={current()}
    onclose={() => (offer = null)}
    onskip={() => {
      app.prefs.updateSkip = offer!.latest;
      app.savePrefs();
      offer = null;
    }}
  />
{/if}

{#if shortcuts}
  <Modal title="Raccourcis clavier" onclose={() => (shortcuts = false)} width="480px">
    <table class="keys">
      <tbody>
        <tr><td>Ctrl+Maj+T</td><td>Nouveau terminal local</td></tr>
        <tr><td>Ctrl+Maj+N</td><td>Nouvelle session</td></tr>
        <tr><td>Ctrl+Maj+O</td><td>Ouvrir un fichier dans l’éditeur</td></tr>
        <tr><td>Ctrl+S / Ctrl+F / Ctrl+G</td><td>Éditeur : enregistrer / rechercher / aller à la ligne</td></tr>
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
  /* Palette: brun profond #28251F, noisette #58402B, olive brun #62553B, ambre #9C6F3E,
     olive végétal #696B4D, crème #F3EDE0 — light ("crème") and dark ("brun") variants */
  :global(:root) {
    --chrome: #f3ede0;
    --chrome-border: #ddd3bf;
    --chrome-border-dark: #bfb29a;
    --panel: #fbf8f1;
    --hover: #ece2cf;
    --hover-border: #cdb38e;
    --selected: #e6d5b9;
    --text: #28251f;
    --text-disabled: #ada492;
    --muted: #6e6656;
    --accent: #9c6f3e;
    --accent-text: #fbf8f1;
    --accent-2: #696b4d;
    --brand: #58402b;
    --danger: #a8432f;
    --ok: #6b7f3a;
    --warn: #c48a2c;
    --input-bg: #fffdf8;
    --input-border: #c9bda6;
    --btn-bg: #fbf8f1;
    --btn-border: #cfc3ac;
    --toolbar-top: #f3ede0;
    --toolbar-bottom: #f3ede0;
    --titlebar: #fbf8f1;
    --titlebar-text: #28251f;
    --home-bg: #ebe3d3;
    --tab-bg: #e7dfcd;
    --row-border: #ece4d4;
    --ok-bg: #edefe0;
    --ok-border: #b9bd98;
    --warn-bg: #f6e9cf;
    --warn-border: #d8b57a;
    --log-bg: #28251f;
    --log-text: #f3ede0;
    --log-time: #d2a868;
    --scroll-thumb: #cdbfa6;
    --scroll-thumb-hover: #b5a78c;
    --switch-off: #d8cfbe;
    --shadow: rgba(40, 37, 31, 0.18);
    --backdrop: rgba(40, 37, 31, 0.35);
    --pane-head: #e7dfcd;
    --pane-head-on: #e6d5b9;
    --terms-bg: #28251f;
    --mono: "Cascadia Mono", Consolas, "DejaVu Sans Mono", monospace;
    /* aliases used by older components */
    --fg: var(--text);
    --bg: var(--panel);
    --border: var(--chrome-border);
    color-scheme: light;
  }
  :global(:root[data-theme="dark"]) {
    --chrome: #28251f;
    --chrome-border: #3d372d;
    --chrome-border-dark: #4f473a;
    --panel: #302c25;
    --hover: #3b352c;
    --hover-border: #62553b;
    --selected: #4a3a28;
    --text: #f3ede0;
    --text-disabled: #6f675a;
    --muted: #b3a993;
    --accent: #b98348;
    --accent-text: #28251f;
    --accent-2: #8e9064;
    --brand: #c99a63;
    --danger: #d06a55;
    --ok: #9aa85e;
    --warn: #d9a24a;
    --input-bg: #1f1c17;
    --input-border: #4f473a;
    --btn-bg: #36312a;
    --btn-border: #4f473a;
    --toolbar-top: #28251f;
    --toolbar-bottom: #28251f;
    --titlebar: #302c25;
    --titlebar-text: #f3ede0;
    --home-bg: #24211c;
    --tab-bg: #2e2a23;
    --row-border: #38332b;
    --ok-bg: #2f3226;
    --ok-border: #696b4d;
    --warn-bg: #3a2f1e;
    --warn-border: #9c6f3e;
    --log-bg: #1a1814;
    --log-text: #f3ede0;
    --log-time: #c99a63;
    --scroll-thumb: #4f473a;
    --scroll-thumb-hover: #62553b;
    --switch-off: #4f473a;
    --shadow: rgba(0, 0, 0, 0.5);
    --backdrop: rgba(0, 0, 0, 0.5);
    --pane-head: #36312a;
    --pane-head-on: #4a3a28;
    --terms-bg: #1a1814;
    color-scheme: dark;
  }
  /* Cyberpunk: night blue-violet, neon cyan / magenta / yellow */
  :global(:root[data-skin="cyber"]) {
    --chrome: #0d0e1f;
    --chrome-border: #2a2d5c;
    --chrome-border-dark: #3d4188;
    --panel: #121430;
    --hover: #1c1f47;
    --hover-border: #00e5ff;
    --selected: #2b1458;
    --text: #e6f7ff;
    --text-disabled: #5b6190;
    --muted: #8f9ad0;
    --accent: #00e5ff;
    --accent-text: #06071a;
    --accent-2: #ff2a8a;
    --brand: #fcee0a;
    --danger: #ff3b6b;
    --ok: #3dffa2;
    --warn: #fcee0a;
    --input-bg: #080916;
    --input-border: #3d4188;
    --btn-bg: #181a3d;
    --btn-border: #3d4188;
    --toolbar-top: #14163a;
    --toolbar-bottom: #0d0e1f;
    --titlebar: #121430;
    --titlebar-text: #e6f7ff;
    --home-bg: #090a18;
    --tab-bg: #101229;
    --row-border: #1d2048;
    --ok-bg: #0c2a2a;
    --ok-border: #1f8f6a;
    --warn-bg: #2c2a0c;
    --warn-border: #a89f1a;
    --log-bg: #05060f;
    --log-text: #c8f6ff;
    --log-time: #ff2a8a;
    --scroll-thumb: #3d4188;
    --scroll-thumb-hover: #00e5ff;
    --switch-off: #2a2d5c;
    --shadow: rgba(0, 229, 255, 0.18);
    --backdrop: rgba(4, 4, 16, 0.65);
    --pane-head: #181a3d;
    --pane-head-on: #2b1458;
    --terms-bg: #05060f;
    color-scheme: dark;
  }
  :global(:root[data-skin="cyber"] button.primary) {
    box-shadow: 0 0 8px rgba(0, 229, 255, 0.55);
  }
  :global(:root[data-skin="cyber"] .modal) {
    border-color: #00e5ff;
    box-shadow:
      0 0 0 1px rgba(255, 42, 138, 0.35),
      0 0 24px rgba(0, 229, 255, 0.25);
  }
  :global(:root[data-skin="cyber"] :is(input, select, textarea):focus) {
    outline: 1px solid #ff2a8a;
    box-shadow: 0 0 6px rgba(255, 42, 138, 0.5);
  }
  /* the amber icons turn cyan / violet */
  :global(:root[data-skin="cyber"] svg.ico) {
    filter: hue-rotate(150deg) saturate(1.8) brightness(1.1);
  }
  /* Grey, dark: the classic dark look of MobaXterm (neutral greys, blue highlights) */
  :global(:root[data-skin="graphite"]) {
    --chrome: #3c3f41;
    --chrome-border: #2b2b2b;
    --chrome-border-dark: #5a5d5f;
    --panel: #2b2b2b;
    --hover: #4c5052;
    --hover-border: #6e7275;
    --selected: #2f65ca;
    --text: #dcdcdc;
    --text-disabled: #777b7e;
    --muted: #a3a7aa;
    --accent: #4a88c7;
    --accent-text: #ffffff;
    --accent-2: #6a9955;
    --brand: #8ab4e8;
    --danger: #e0605a;
    --ok: #7bb662;
    --warn: #d9a441;
    --input-bg: #45494a;
    --input-border: #646769;
    --btn-bg: #4c5052;
    --btn-border: #5e6163;
    --toolbar-top: #45484a;
    --toolbar-bottom: #3c3f41;
    --titlebar: #313335;
    --titlebar-text: #dcdcdc;
    --home-bg: #2b2b2b;
    --tab-bg: #35383a;
    --row-border: #35383a;
    --ok-bg: #2f3b2c;
    --ok-border: #5c7a4f;
    --warn-bg: #45391f;
    --warn-border: #8f7334;
    --log-bg: #1e1e1e;
    --log-text: #dcdcdc;
    --log-time: #8ab4e8;
    --scroll-thumb: #5e6163;
    --scroll-thumb-hover: #777b7e;
    --switch-off: #5e6163;
    --shadow: rgba(0, 0, 0, 0.5);
    --backdrop: rgba(0, 0, 0, 0.5);
    --pane-head: #45484a;
    --pane-head-on: #2f65ca;
    --terms-bg: #1e1e1e;
    color-scheme: dark;
  }
  /* Grey, light: the classic Windows-grey look */
  :global(:root[data-skin="graphite-light"]) {
    --chrome: #f0f0f0;
    --chrome-border: #d0d0d0;
    --chrome-border-dark: #a8a8a8;
    --panel: #ffffff;
    --hover: #e5f1fb;
    --hover-border: #7ab4e6;
    --selected: #cce4f7;
    --text: #1e1e1e;
    --text-disabled: #9a9a9a;
    --muted: #5f6368;
    --accent: #0067c0;
    --accent-text: #ffffff;
    --accent-2: #3f7f3f;
    --brand: #1f4e79;
    --danger: #c42b1c;
    --ok: #2e7d32;
    --warn: #b7791f;
    --input-bg: #ffffff;
    --input-border: #b4b4b4;
    --btn-bg: #fdfdfd;
    --btn-border: #bdbdbd;
    --toolbar-top: #f7f7f7;
    --toolbar-bottom: #e6e6e6;
    --titlebar: #ffffff;
    --titlebar-text: #1e1e1e;
    --home-bg: #e8e8e8;
    --tab-bg: #e2e2e2;
    --row-border: #ececec;
    --ok-bg: #e6f2e6;
    --ok-border: #9cc59c;
    --warn-bg: #fbf0d9;
    --warn-border: #dcb766;
    --log-bg: #1e1e1e;
    --log-text: #e6e6e6;
    --log-time: #8ab4e8;
    --scroll-thumb: #c2c2c2;
    --scroll-thumb-hover: #a0a0a0;
    --switch-off: #cfcfcf;
    --shadow: rgba(0, 0, 0, 0.2);
    --backdrop: rgba(0, 0, 0, 0.3);
    --pane-head: #e2e2e2;
    --pane-head-on: #cce4f7;
    --terms-bg: #1e1e1e;
    color-scheme: light;
  }
  :global(html, body) {
    margin: 0;
    height: 100%;
    background: var(--chrome);
    color: var(--text);
    font: 13px/1.45 "Segoe UI", system-ui, "Noto Sans", sans-serif;
    overflow: hidden;
  }
  :global(button) {
    font: inherit;
    color: var(--text);
    background: var(--btn-bg);
    border: 1px solid var(--btn-border);
    border-radius: 4px;
    padding: 3px 10px;
    cursor: default;
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
  :global(button:hover:not(:disabled)) {
    background: var(--hover);
    border-color: var(--hover-border);
  }
  :global(button:disabled) {
    color: var(--text-disabled);
  }
  :global(button.primary) {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-text);
  }
  :global(button.primary:hover:not(:disabled)) {
    background: var(--brand);
    border-color: var(--brand);
    color: var(--accent-text);
  }
  :global(button.primary:disabled) {
    opacity: 0.5;
    color: var(--accent-text);
  }
  :global(button.danger-btn) {
    border-color: var(--danger);
    color: var(--danger);
  }
  :global(input, select, textarea) {
    font: inherit;
    color: var(--text);
    background: var(--input-bg);
    border: 1px solid var(--input-border);
    border-radius: 4px;
    padding: 3px 6px;
    min-width: 0;
  }
  :global(input[type="checkbox"], input[type="radio"]) {
    accent-color: var(--accent);
  }
  :global(input:disabled, select:disabled) {
    background: var(--chrome);
    color: var(--text-disabled);
  }
  :global(input:focus, select:focus, textarea:focus) {
    outline: none;
    border-color: var(--accent);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 25%, transparent);
  }
  :global(::-webkit-scrollbar) {
    width: 10px;
    height: 10px;
  }
  :global(::-webkit-scrollbar-track),
  :global(::-webkit-scrollbar-corner) {
    background: transparent;
  }
  :global(::-webkit-scrollbar-thumb) {
    background: var(--scroll-thumb);
    border: 2px solid transparent;
    background-clip: padding-box;
    border-radius: 5px;
  }
  :global(::-webkit-scrollbar-thumb:hover) {
    background: var(--scroll-thumb-hover);
    background-clip: padding-box;
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
    gap: 2px;
    background: var(--chrome);
    border-bottom: 1px solid var(--chrome-border);
    overflow-x: auto;
    overflow-y: hidden;
    height: 32px;
    padding: 4px 4px 0;
    flex: 0 0 auto;
  }
  .tabbar::-webkit-scrollbar {
    height: 0;
  }
  .tab {
    display: flex;
    align-items: center;
    gap: 5px;
    height: 27px;
    padding: 0 4px 0 10px;
    border: 1px solid transparent;
    border-bottom: none;
    border-radius: 6px 6px 0 0;
    background: transparent;
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
    border-color: var(--chrome-border);
    margin-bottom: -1px;
    height: 28px;
    box-shadow: inset 0 -2px 0 var(--accent);
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
  .tname .dirty {
    color: var(--accent);
  }
  .x {
    border: none;
    background: none;
    padding: 3px;
    color: var(--muted);
    border-radius: 2px;
  }
  .x:hover:not(:disabled) {
    background: var(--danger);
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
    background: var(--ok);
  }
  .dot.connecting {
    background: var(--warn);
  }
  .dot.error,
  .dot.closed {
    background: var(--danger);
  }
  .terms {
    position: relative;
    flex: 1;
    min-height: 0;
    background: var(--terms-bg);
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
  .tmain {
    position: absolute;
    inset: 0;
  }
  .tmain.withinfo {
    bottom: 23px;
  }
  .tmain.withbar {
    top: 31px;
  }
  .ttop {
    position: absolute;
    left: 0;
    right: 0;
    top: 0;
  }
  .tinfo {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
  }
  .viewmsg {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--panel);
    color: var(--muted);
  }
  .termwrap.pane {
    inset: auto;
    box-sizing: border-box;
    border: 1px solid var(--chrome-border-dark);
  }
  .termwrap.pane.focus {
    border-color: var(--accent);
  }
  .termwrap.empty {
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--muted);
    background: var(--terms-bg);
  }
  .phead {
    display: flex;
    align-items: center;
    gap: 5px;
    height: 20px;
    padding: 0 6px;
    background: var(--pane-head);
    color: var(--text);
    font-size: 11.5px;
    user-select: none;
  }
  .pane.focus .phead {
    background: var(--pane-head-on);
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
    background: var(--warn-bg);
    border-bottom: 1px solid var(--warn-border);
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
    color: var(--muted);
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
  .msg p.upd {
    white-space: normal;
    border-top: 1px solid var(--border);
    padding-top: 8px;
  }
  button.link {
    background: none;
    border: none;
    padding: 0;
    color: var(--accent);
    text-decoration: underline;
    cursor: pointer;
    font: inherit;
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
