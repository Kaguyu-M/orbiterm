// Global application state shared by every component (Svelte 5 runes).
import {
  api,
  asApiError,
  describe,
  newSession,
  protocol,
  viewOf,
  type Folder,
  type HostKeyPrompt,
  type LocalShell,
  type Macro,
  type MacroStep,
  type Session,
  type Store,
  type Tunnel,
  type TunnelStatus,
  type XServerStatus,
  type ServerKind,
  type ServerStatus,
} from "./api";

import { terminals } from "./Terminal.svelte";

type Secrets ={ password?: string; passphrase?: string; jump_password?: string; remember?: boolean };
import type { MenuItem } from "./MenuBar.svelte";

export type TabState = "connecting" | "open" | "closed" | "error";

export interface Tab {
  key: string;
  session: Session;
  title: string;
  connId: string | null;
  status: string;
  state: TabState;
}

type TextAsk = {
  title: string;
  value: string;
  secret: boolean;
  check?: string;
  checked: boolean;
  resolve: (v: { value: string; checked: boolean } | null) => void;
};
type ConfirmAsk = { message: string; resolve: (v: boolean) => void };

export const HOME = "home";
export type SideTab = "sessions" | "tools" | "macros" | "sftp";

const RECENT_KEY = "orbiterm.recent";
const PREFS_KEY = "orbiterm.prefs";

export interface Prefs {
  sidebar: boolean;
  toolbar: boolean;
  menubar: boolean;
  statusbar: boolean;
  sidebarWidth: number;
  // général
  confirmQuit: boolean;
  defaultShell: string;
  // terminal (defaults for new sessions + behaviour of every terminal)
  termFont: string;
  termFontSize: number;
  termTheme: string;
  termScrollback: number;
  cursorStyle: "block" | "underline" | "bar";
  cursorBlink: boolean;
  copyOnSelect: boolean;
  rightClickPaste: boolean;
  visualBell: boolean;
  // SSH
  keepalive: number;
  sshX11: boolean;
  sshCompression: boolean;
  sshSftp: boolean;
  /** information bar (CPU, RAM…) under SSH terminals */
  sshMonitor: boolean;
  /** bar above remote terminals (copy / paste, SFTP, close…) */
  termBar: boolean;
  // X11
  xAutoStart: boolean;
  // apparence de l'interface
  uiTheme: "light" | "dark" | "system";
}

export const DEFAULT_PREFS: Prefs = {
  sidebar: true,
  toolbar: true,
  menubar: true,
  statusbar: true,
  sidebarWidth: 280,
  confirmQuit: true,
  defaultShell: "",
  termFont: "Cascadia Mono, Consolas, 'DejaVu Sans Mono', monospace",
  termFontSize: 14,
  termTheme: "orbiterm",
  termScrollback: 10000,
  cursorStyle: "block",
  cursorBlink: true,
  copyOnSelect: true,
  rightClickPaste: true,
  visualBell: false,
  keepalive: 30,
  sshX11: false,
  sshCompression: false,
  sshSftp: true,
  sshMonitor: true,
  termBar: true,
  xAutoStart: true,
  uiTheme: "system",
};

/** Screen layouts of the terminal area (Split button). */
export type SplitMode = "single" | "v2" | "h2" | "grid4" | "all";
export const SPLIT_LABELS: Record<SplitMode, string> = {
  single: "Un seul terminal",
  v2: "Deux terminaux côte à côte",
  h2: "Deux terminaux l'un sur l'autre",
  grid4: "Quatre terminaux (grille 2×2)",
  all: "Tous les terminaux en mosaïque",
};
const SPLIT_SLOTS: Record<SplitMode, number> = { single: 1, v2: 2, h2: 2, grid4: 4, all: 0 };

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

function loadJson<T>(key: string, fallback: T): T {
  try {
    const v = localStorage.getItem(key);
    return v ? { ...fallback, ...JSON.parse(v) } : fallback;
  } catch {
    return fallback;
  }
}

/** Prefs saved by older versions: adopt the new palette once. */
function migratePrefs(p: Prefs): Prefs {
  try {
    const raw = JSON.parse(localStorage.getItem(PREFS_KEY) ?? "{}");
    if (raw && !("uiTheme" in raw) && p.termTheme === "dark") p.termTheme = "orbiterm";
  } catch {
    /* ignore */
  }
  return p;
}

/** Recently opened sessions (saved or quick-connect), newest first. */
export interface Recent {
  session: Session;
  at: number;
}

class App {
  store: Store = $state({ version: 1, folders: [], sessions: [], tunnels: [], macros: [] });
  tabs: Tab[] = $state([]);
  activeKey: string = $state(HOME);
  sideTab: SideTab = $state("sessions");
  editing: Session | null = $state(null);
  shells: LocalShell[] = $state([]);
  recent: Recent[] = $state([]);
  /** Secrets typed in a tab (memory only, cleared when the tab closes). */
  private typedSecrets = new Map<string, Secrets>();
  /** Secrets to use for the first connection of a new tab. */
  private pendingSecrets = new Map<string, Secrets>();
  prefs: Prefs = $state(migratePrefs(loadJson(PREFS_KEY, DEFAULT_PREFS)));
  showSettings = $state(false);
  menuOpen = $state(false);
  showImport = $state(false);

  // split screen
  split: SplitMode = $state("single");
  /** tab keys shown in the split panes, in pane order */
  panes: string[] = $state([]);
  private lastPane = 0;
  // MultiExec: typing in one terminal is sent to every included terminal
  multiExec = $state(false);
  meExcluded: string[] = $state([]);
  private splitBeforeMe: SplitMode = "single";
  // macro recording
  recording: { tabKey: string; steps: MacroStep[]; last: number } | null = $state(null);
  playing = $state(false);

  textAsk: TextAsk | null = $state(null);
  confirmAsk: ConfirmAsk | null = $state(null);
  alertText = $state("");
  infoText = $state("");
  hostKeys: HostKeyPrompt[] = $state([]);
  tunnels: TunnelStatus[] = $state([]);
  showTunnels = $state(false);
  servers: ServerStatus[] = $state([]);
  showServers = $state(false);
  serverFocus: ServerKind = $state("http");
  /** network tool window: port scanner, open ports, Wake-on-LAN */
  netTool: "scan" | "ports" | "wol" | null = $state(null);
  xserver: XServerStatus | null = $state(null);
  /** Open context menu (right click), if any. */
  ctx: { x: number; y: number; items: MenuItem[] } | null = $state(null);

  active = $derived(this.tabs.find((t) => t.key === this.activeKey) ?? null);
  /** Something is drawn above the tab area (native browser webviews must hide). */
  overlay = $derived(
    !!this.ctx || !!this.editing || !!this.textAsk || !!this.confirmAsk || !!this.alertText || !!this.infoText ||
      this.showSettings || this.showImport || this.showTunnels || this.showServers || !!this.netTool || this.menuOpen || this.hostKeys.length > 0,
  );
  /** SFTP side tab exists only while the active terminal is a live SSH connection. */
  hasSftp = $derived(
    !!this.active &&
      this.active.state === "open" &&
      !!this.active.connId &&
      this.active.session.kind === "ssh" &&
      this.active.session.sftp_enabled,
  );

  constructor() {
    try {
      this.recent = JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]");
    } catch {
      this.recent = [];
    }
  }

  savePrefs() {
    try {
      localStorage.setItem(PREFS_KEY, JSON.stringify($state.snapshot(this.prefs)));
    } catch {
      /* private mode */
    }
  }

  // ------------------------------------------------------------ dialogs
  ask(title: string, initial = "", secret = false, check?: string, checked = false) {
    return new Promise<{ value: string; checked: boolean } | null>((resolve) => {
      this.textAsk = { title, value: initial, secret, check, checked, resolve };
    });
  }
  closeAsk(ok: boolean) {
    const t = this.textAsk;
    if (!t) return;
    this.textAsk = null;
    t.resolve(ok ? { value: t.value, checked: t.checked } : null);
  }
  askText = async (title: string, initial: string) => {
    const r = await this.ask(title, initial);
    return r?.value.trim() || null;
  };
  confirm = (message: string) =>
    new Promise<boolean>((resolve) => {
      this.confirmAsk = { message, resolve };
    });
  closeConfirm(v: boolean) {
    const c = this.confirmAsk;
    this.confirmAsk = null;
    c?.resolve(v);
  }
  alert(m: string) {
    this.alertText = m;
  }
  info(m: string) {
    this.infoText = m;
  }

  // ------------------------------------------------------------ sessions
  async reload() {
    this.store = await api.sessionsLoad();
  }

  async init() {
    await this.reload().catch((e) => this.alert(asApiError(e).message));
    this.shells = await api.localShells().catch(() => []);
    await this.refreshX();
    await this.refreshTunnels();
    await this.refreshServers();
    // tunnels marked "start automatically"
    for (const t of this.tunnels.filter((x) => x.autostart && !x.running)) {
      await this.startTunnel(t, true);
    }
  }

  // ------------------------------------------------------------ X server
  async refreshX() {
    this.xserver = await api.xserverStatus().catch(() => null);
  }

  async toggleX() {
    try {
      this.xserver = this.xserver?.running && this.xserver.owned ? await api.xserverStop() : await api.xserverStart();
      if (this.xserver?.running && !this.xserver.owned && this.xserver.kind !== "native")
        this.info("Un serveur X tourne déjà sur ce poste (127.0.0.1:0) : il sera utilisé pour la redirection X11.");
    } catch (e) {
      this.alert(asApiError(e).message);
    }
  }

  // ------------------------------------------------------------ built-in servers
  async refreshServers() {
    this.servers = await api.serversStatus().catch(() => this.servers);
  }

  openServers(kind?: ServerKind) {
    if (kind) this.serverFocus = kind;
    this.refreshServers();
    this.showServers = true;
  }

  async toggleServer(kind: ServerKind) {
    const s = this.servers.find((x) => x.config.kind === kind);
    try {
      if (s?.running) await api.serverStop(kind);
      else await api.serverStart(kind);
    } catch (e) {
      this.alert(asApiError(e).message);
    }
    await this.refreshServers();
  }

  // ------------------------------------------------------------ tunnels (manager)
  async refreshTunnels() {
    this.tunnels = await api.tunnelsList().catch(() => []);
  }

  async saveTunnel(t: Tunnel) {
    try {
      await api.tunnelSave(t);
      await this.refreshTunnels();
    } catch (e) {
      this.alert(asApiError(e).message);
    }
  }

  async deleteTunnel(t: Tunnel) {
    if (!(await this.confirm(`Supprimer le tunnel « ${t.name} » ?`))) return;
    await api.tunnelDelete(t.id).catch((e) => this.alert(asApiError(e).message));
    await this.refreshTunnels();
  }

  async startTunnel(t: Tunnel, quiet = false) {
    const s = this.store.sessions.find((x) => x.id === t.session_id);
    if (!s) {
      if (!quiet) this.alert(`Le tunnel « ${t.name} » utilise une session SSH qui n'existe plus.`);
      return;
    }
    try {
      await this.withSecrets(s, (sec) => api.tunnelStart({ id: t.id, ...sec }));
    } catch (e) {
      const err = asApiError(e);
      if (err.kind !== "cancelled") this.alert(`Tunnel « ${t.name} » : ${err.message}`);
    }
    await this.refreshTunnels();
  }

  async stopTunnel(t: Tunnel) {
    await api.tunnelStop(t.id).catch(() => {});
    await this.refreshTunnels();
  }

  /**
   * Runs `run` and, when the backend needs a secret (password, key passphrase,
   * jump host password), asks for it and tries again.
   */
  async withSecrets<T>(s: Session, run: (sec: Secrets) => Promise<T>, sec: Secrets = {}, note = ""): Promise<T> {
    try {
      return await run(sec);
    } catch (e) {
      const err = asApiError(e);
      const who = `${s.username ? s.username + "@" : ""}${s.host}`;
      const canStore = !!s.id && s.remember_password;
      const cancelled = { kind: "cancelled", message: "Connexion annulée." };
      const check = canStore ? "Mémoriser dans le trousseau du système" : undefined;
      switch (err.kind) {
        case "need_password": {
          const r = await this.ask(`${note}Mot de passe pour ${who}`, "", true, check, canStore);
          if (!r) throw cancelled;
          return this.withSecrets(s, run, { ...sec, password: r.value, remember: r.checked });
        }
        case "need_jump_password": {
          const ju = s.extra?.jump_user || s.username;
          const r = await this.ask(`${note}Mot de passe du rebond SSH ${ju ? ju + "@" : ""}${s.extra?.jump_host}`, "", true, check, canStore);
          if (!r) throw cancelled;
          return this.withSecrets(s, run, { ...sec, jump_password: r.value, remember: r.checked });
        }
        case "need_passphrase": {
          const r = await this.ask(`${note}Passphrase de la clé ${s.key_path}`, "", true);
          if (!r) throw cancelled;
          return this.withSecrets(s, run, { ...sec, passphrase: r.value });
        }
        case "auth_failed":
          if (sec.password !== undefined) return this.withSecrets(s, run, { ...sec, password: undefined }, "⚠ Mot de passe refusé — ");
          if (sec.passphrase !== undefined) return this.withSecrets(s, run, { ...sec, passphrase: undefined }, "⚠ Clé refusée — ");
          break;
        case "jump_auth_failed":
          if (sec.jump_password !== undefined)
            return this.withSecrets(s, run, { ...sec, jump_password: undefined }, "⚠ Mot de passe du rebond refusé — ");
          break;
        case "ssh":
          if (sec.passphrase !== undefined && /decrypt|passphrase|crypto/i.test(err.message))
            return this.withSecrets(s, run, { ...sec, passphrase: undefined }, "⚠ Passphrase incorrecte — ");
          break;
      }
      throw err;
    }
  }

  /** A new session pre-filled with the global settings. */
  blank(folder = "", kind = "ssh"): Session {
    const s = newSession(folder, kind);
    const p = this.prefs;
    s.terminal = { font_family: p.termFont, font_size: p.termFontSize, theme: p.termTheme, scrollback: p.termScrollback };
    if (kind === "ssh") {
      s.sftp_enabled = p.sshSftp;
      if (p.sshX11) s.extra.x11 = "1";
      if (p.sshCompression) s.extra.compression = "1";
    }
    return s;
  }

  newSession(folder = "", kind = "ssh") {
    this.editing = this.blank(folder, kind);
  }

  // ------------------------------------------------------------ import
  async importFrom(source: string, path?: string) {
    const rep = await api.importRun(source, path);
    await this.reload();
    return rep;
  }

  // ------------------------------------------------------------ split screen / MultiExec
  setSplit(mode: SplitMode) {
    if (this.multiExec && mode !== "all") this.multiExec = false;
    this.split = mode;
    this.syncPanes();
  }

  /** Keeps `panes` consistent with the open tabs and the active tab. */
  syncPanes() {
    const keys = this.tabs.map((t) => t.key);
    let p = this.panes.filter((k) => keys.includes(k));
    if (this.split === "all") p = keys;
    else {
      const n = SPLIT_SLOTS[this.split];
      const a = this.activeKey;
      if (a !== HOME && keys.includes(a)) {
        const i = p.indexOf(a);
        if (i >= 0) this.lastPane = i;
        else if (p.length < n) {
          p.push(a);
          this.lastPane = p.length - 1;
        } else p[Math.min(this.lastPane, n - 1)] = a;
      }
      for (const k of keys) {
        if (p.length >= n) break;
        if (!p.includes(k)) p.push(k);
      }
      p = p.slice(0, n);
    }
    if (p.join() !== this.panes.join()) this.panes = p;
  }

  /** Number of pane slots of the current layout (0 = single). */
  get slots() {
    return this.split === "single" ? 0 : this.split === "all" ? this.panes.length : SPLIT_SLOTS[this.split];
  }

  toggleMultiExec() {
    if (this.multiExec) {
      this.multiExec = false;
      this.setSplit(this.splitBeforeMe);
    } else {
      this.splitBeforeMe = this.split;
      this.meExcluded = [];
      this.split = "all";
      this.multiExec = true;
      this.syncPanes();
      if (this.activeKey === HOME && this.tabs.length) this.activeKey = this.tabs[0].key;
    }
  }

  meIncluded(key: string) {
    return !this.meExcluded.includes(key);
  }

  meToggle(key: string) {
    this.meExcluded = this.meIncluded(key) ? [...this.meExcluded, key] : this.meExcluded.filter((k) => k !== key);
  }

  /** Terminals that receive MultiExec input / macros. */
  targets(): Tab[] {
    const live = this.tabs.filter((t) => t.state === "open" && t.connId && viewOf(t.session) === "terminal");
    if (this.multiExec) return live.filter((t) => this.meIncluded(t.key));
    return live.filter((t) => t.key === this.activeKey);
  }

  /** Called with every chunk typed in a terminal (the terminal already sent it to its own connection). */
  onInput(tabKey: string, data: string) {
    const r = this.recording;
    if (r && r.tabKey === tabKey) {
      const now = Date.now();
      r.steps.push({ data, delay_ms: r.steps.length ? now - r.last : 0 });
      r.last = now;
    }
    if (this.multiExec && this.meIncluded(tabKey)) {
      for (const t of this.targets()) if (t.key !== tabKey) api.termWrite(t.connId!, data).catch(() => {});
    }
  }

  // ------------------------------------------------------------ macros
  startRecording() {
    if (!this.active || this.active.state !== "open") return this.alert("Ouvre et sélectionne d'abord un terminal connecté.");
    this.recording = { tabKey: this.active.key, steps: [], last: Date.now() };
  }

  async stopRecording() {
    const r = this.recording;
    this.recording = null;
    if (!r) return;
    if (!r.steps.length) return this.info("Aucune frappe n'a été enregistrée.");
    const name = await this.askText("Nom de la macro", `Macro ${this.store.macros.length + 1}`);
    if (!name) return;
    await api.macroSave({ id: "", name, steps: $state.snapshot(r.steps) as MacroStep[] });
    await this.reload();
    this.sideTab = "macros";
    this.prefs.sidebar = true;
  }

  async playMacro(m: Macro) {
    const targets = this.targets();
    if (!targets.length) return this.alert("Aucun terminal connecté pour rejouer la macro.");
    this.playing = true;
    try {
      for (const st of m.steps) {
        if (st.delay_ms) await sleep(Math.min(st.delay_ms, 1500));
        for (const t of targets) await api.termWrite(t.connId!, st.data).catch(() => {});
      }
    } finally {
      this.playing = false;
    }
    terminals.get(this.activeKey)?.focus();
  }

  async renameMacro(m: Macro) {
    const name = await this.askText("Renommer la macro", m.name);
    if (!name) return;
    await api.macroSave({ ...($state.snapshot(m) as Macro), name });
    await this.reload();
  }

  async deleteMacro(m: Macro) {
    if (!(await this.confirm(`Supprimer la macro « ${m.name} » ?`))) return;
    await api.macroDelete(m.id);
    await this.reload();
  }

  async saveSession(s: Session, openAfter = false) {
    this.editing = null;
    try {
      const saved = await api.sessionSave(s);
      await this.reload();
      if (openAfter) this.openSession(saved);
    } catch (e) {
      this.alert(asApiError(e).message);
    }
  }

  async duplicate(s: Session) {
    const copy = structuredClone($state.snapshot(s)) as Session;
    copy.id = "";
    copy.name = `${s.name} (copie)`;
    await this.saveSession(copy);
  }

  async deleteSession(s: Session) {
    if (!(await this.confirm(`Supprimer la session « ${s.name} » ?`))) return;
    await api.sessionDelete(s.id);
    this.recent = this.recent.filter((r) => r.session.id !== s.id);
    this.persistRecent();
    await this.reload();
  }

  async newFolder(parent: string) {
    const name = await this.askText("Nom du dossier", "");
    if (!name) return;
    await api.folderSave({ id: "", name, parent });
    await this.reload();
  }

  async renameFolder(f: Folder) {
    const name = await this.askText("Renommer le dossier", f.name);
    if (!name) return;
    await api.folderSave({ ...f, name });
    await this.reload();
  }

  async deleteFolder(f: Folder) {
    if (!(await this.confirm(`Supprimer le dossier « ${f.name} », ses sous-dossiers et toutes leurs sessions ?`)))
      return;
    await api.folderDelete(f.id);
    await this.reload();
  }

  private persistRecent() {
    try {
      localStorage.setItem(RECENT_KEY, JSON.stringify($state.snapshot(this.recent)));
    } catch {
      /* ignore */
    }
  }

  private remember(s: Session) {
    const key = (x: Session) => (x.id ? x.id : `${x.kind}|${x.username}|${x.host}|${x.port}|${JSON.stringify(x.extra)}`);
    const k = key(s);
    this.recent = [{ session: structuredClone($state.snapshot(s)) as Session, at: Date.now() }, ...this.recent.filter((r) => key(r.session) !== k)].slice(0, 15);
    this.persistRecent();
  }

  // ------------------------------------------------------------ tabs / connections
  openSession(s: Session, secrets?: Secrets) {
    const p = protocol(s.kind);
    if (!p.ready) {
      this.info(`Le type de session « ${p.label} » arrive dans une prochaine étape.`);
      return;
    }
    const session = structuredClone($state.snapshot(s)) as Session;
    if (!session.name) session.name = describe(session);
    const tab: Tab = {
      key: crypto.randomUUID(),
      session,
      title: session.name,
      connId: null,
      status: "",
      state: "connecting",
    };
    this.tabs.push(tab);
    this.activeKey = tab.key;
    if (secrets) this.pendingSecrets.set(tab.key, secrets);
    this.remember(session);
    this.connect(this.tabs[this.tabs.length - 1]);
  }

  /** Opens a local terminal with the given shell id (default shell when empty). */
  startLocalTerminal(shellId = "") {
    const sh = this.shells.find((x) => x.id === (shellId || this.prefs.defaultShell)) ?? this.shells[0];
    const s = this.blank("", "shell");
    if (sh?.id.startsWith("wsl:")) {
      s.kind = "wsl";
      s.extra = { distro: sh.id.slice(4) };
    } else {
      s.extra = { shell: sh?.id ?? "" };
    }
    s.name = sh?.label ?? "Terminal local";
    this.openSession(s);
  }

  /**
   * Quick connect, like a command line: `user@host`, `host:port`,
   * `ssh user@host -p 2222`, `telnet host 23`, `rlogin user@host`,
   * `mosh user@host`, `serial COM3 9600`, `wsl Ubuntu`, `cmd`, `powershell`.
   */
  quickConnect(text: string) {
    const parts = text.trim().split(/\s+/).filter(Boolean);
    if (!parts.length) return;
    const known = ["ssh", "telnet", "rlogin", "mosh", "serial", "wsl"];
    let kind = "ssh";
    if (known.includes(parts[0].toLowerCase())) kind = parts.shift()!.toLowerCase();
    else {
      const sh = this.shells.find((x) => x.id === parts[0].toLowerCase());
      if (sh && parts.length === 1) return this.startLocalTerminal(sh.id);
    }
    const s = this.blank("", kind);
    if (kind === "serial") {
      s.extra = { serial_port: parts[0] ?? "", baud: parts[1] ?? "115200" };
    } else if (kind === "wsl") {
      s.extra = { distro: parts[0] ?? "" };
    } else {
      let target = parts.shift() ?? "";
      for (let i = 0; i < parts.length; i++) {
        if (parts[i] === "-p" && parts[i + 1]) s.port = Number(parts[++i]) || s.port;
        else if (parts[i] === "-l" && parts[i + 1]) s.username = parts[++i];
        else if (/^\d+$/.test(parts[i])) s.port = Number(parts[i]);
      }
      const at = target.lastIndexOf("@");
      if (at > 0) {
        s.username = target.slice(0, at);
        target = target.slice(at + 1);
      }
      const m = /^\[(.+)\]:(\d+)$/.exec(target) ?? /^([^:]+):(\d+)$/.exec(target);
      if (m) {
        target = m[1];
        s.port = Number(m[2]);
      }
      s.host = target;
      if (!s.host) return;
    }
    s.remember_password = false;
    this.openSession(s);
  }

  async connect(tab: Tab): Promise<void> {
    const s = tab.session;
    tab.state = "connecting";
    if (s.kind === "browser") {
      // no backend connection: the BrowserView owns its webview
      tab.state = "open";
      tab.status = "";
      return;
    }
    tab.status = protocol(s.kind).remote ? `Connexion à ${describe(s)}…` : `Démarrage : ${describe(s)}…`;
    try {
      const snap = $state.snapshot(s) as Session;
      if (snap.kind === "rdp") {
        // "adapt to the window": size of the tab area, in physical pixels
        const r = document.querySelector(".terms")?.getBoundingClientRect();
        const k = window.devicePixelRatio || 1;
        // (the embedded viewer keeps a 28 px toolbar above the desktop)
        const bar = viewOf(snap) === "rdp" ? (snap.extra?.rdp_monitor === "0" ? 30 : 53) : 0;
        if (r) snap.extra = { ...snap.extra, rdp_w: String(Math.round(r.width * k)), rdp_h: String(Math.round((r.height - bar) * k)) };
      }
      if (snap.kind === "ssh") {
        snap.extra = { ...snap.extra, keepalive: String(this.prefs.keepalive) };
        // like the original: the X server is started on demand for X11 sessions
        const x = this.xserver;
        if (snap.extra.x11 === "1" && this.prefs.xAutoStart && x && !x.running && x.kind && x.kind !== "native")
          this.xserver = await api.xserverStart().catch(() => x);
      }
      // size is corrected by the terminal right after attaching
      const initial = this.pendingSecrets.get(tab.key) ?? {};
      this.pendingSecrets.delete(tab.key);
      const connId = await this.withSecrets(
        s,
        async (sec) => {
          const id = await api.termOpen({ session: snap, cols: 120, rows: 32, ...sec });
          // kept in memory only, to open a separate SFTP session later
          if (sec.password !== undefined || sec.jump_password !== undefined) this.typedSecrets.set(tab.key, { ...sec });
          return id;
        },
        initial,
      );
      tab.connId = connId;
      tab.state = "open";
      tab.status = "";
      if (s.kind === "ssh" && s.sftp_enabled && tab.key === this.activeKey && this.prefs.sidebar) this.sideTab = "sftp";
    } catch (e) {
      const err = asApiError(e);
      tab.state = "error";
      tab.status = err.kind === "cancelled" ? err.message : `Échec : ${err.message}`;
    }
  }

  reconnect(tab: Tab) {
    const old = tab.connId;
    tab.connId = null;
    if (old) api.termClose(old).catch(() => {});
    this.connect(tab);
  }

  async closeTab(tab: Tab) {
    this.typedSecrets.delete(tab.key);
    if (tab.connId) await api.termClose(tab.connId).catch(() => {});
    if (tab.session.kind === "browser") await api.browserClose(`browser-${tab.key}`).catch(() => {});
    const i = this.tabs.findIndex((t) => t.key === tab.key);
    if (i < 0) return;
    this.tabs.splice(i, 1);
    if (this.activeKey === tab.key) this.activeKey = this.tabs[Math.min(i, this.tabs.length - 1)]?.key ?? HOME;
  }

  async closeAllTabs() {
    for (const t of [...this.tabs]) await this.closeTab(t);
  }

  duplicateTab(tab: Tab) {
    this.openSession(tab.session);
  }

  /**
   * From an SSH tab: creates (or reuses) a saved SFTP session to the same
   * server, with the same login, key and jump host, and opens it. The
   * password is saved in the system keyring for the new session.
   */
  async openSeparateSftp(tab: Tab) {
    const src = $state.snapshot(tab.session) as Session;
    if (src.kind !== "ssh" && src.kind !== "sftp") return;
    const same = (x: Session) =>
      x.kind === "sftp" &&
      x.host.toLowerCase() === src.host.toLowerCase() &&
      x.port === src.port &&
      x.username === src.username &&
      (x.extra?.jump_host ?? "") === (src.extra?.jump_host ?? "");
    let target = this.store.sessions.find(same);
    if (!target) {
      const s = this.blank(src.folder, "sftp");
      const who = `${src.username ? src.username + "@" : ""}${src.host}`;
      s.name = `${src.id && src.name ? src.name : who} (SFTP)`;
      s.host = src.host;
      s.port = src.port;
      s.username = src.username;
      s.auth = src.auth;
      s.key_path = src.key_path;
      s.remember_password = true;
      for (const k of ["jump_host", "jump_port", "jump_user", "jump_key", "compression"])
        if (src.extra?.[k]) s.extra[k] = src.extra[k];
      target = await api.sessionSave(s);
      await this.reload();
    } else if (!target.remember_password) {
      target = await api.sessionSave({ ...$state.snapshot(target), remember_password: true } as Session);
      await this.reload();
    }
    // password: typed in this tab (memory), else copied inside the keyring
    const typed = this.typedSecrets.get(tab.key);
    let secrets: Secrets | undefined;
    if (typed && (typed.password !== undefined || typed.jump_password !== undefined)) {
      secrets = { password: typed.password, jump_password: typed.jump_password, remember: true };
    } else if (src.id) {
      await api.passwordCopy(src.id, target.id).catch(() => false);
    }
    this.openSession(target, secrets);
  }

  async renameTab(tab: Tab) {
    const name = await this.askText("Renommer l'onglet", tab.title);
    if (name) tab.title = name;
  }

  nextTab(dir: 1 | -1) {
    const keys = [HOME, ...this.tabs.map((t) => t.key)];
    const i = keys.indexOf(this.activeKey);
    this.activeKey = keys[(i + dir + keys.length) % keys.length];
  }

  onConnectionClosed(connId: string, reason = "") {
    const t = this.tabs.find((x) => x.connId === connId);
    if (t) {
      // views without a terminal (RDP, VNC…) show it from tab.status
      t.status = reason;
      // the terminal prints the reason; release the backend connection
      t.state = "closed";
      api.termClose(connId).catch(() => {});
    }
  }

  async answerHostKey(accept: boolean) {
    const hk = this.hostKeys.shift();
    if (hk) await api.hostkeyAnswer(hk.request_id, accept);
  }
}

export const app = new App();
