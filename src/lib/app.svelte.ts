// Global application state shared by every component (Svelte 5 runes).
import {
  api,
  asApiError,
  describe,
  newSession,
  protocol,
  type Folder,
  type HostKeyPrompt,
  type LocalShell,
  type Session,
  type Store,
  type Tunnel,
  type TunnelStatus,
  type XServerStatus,
} from "./api";

type Secrets = { password?: string; passphrase?: string; jump_password?: string; remember?: boolean };
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
}

function loadJson<T>(key: string, fallback: T): T {
  try {
    const v = localStorage.getItem(key);
    return v ? { ...fallback, ...JSON.parse(v) } : fallback;
  } catch {
    return fallback;
  }
}

/** Recently opened sessions (saved or quick-connect), newest first. */
export interface Recent {
  session: Session;
  at: number;
}

class App {
  store: Store = $state({ version: 1, folders: [], sessions: [], tunnels: [] });
  tabs: Tab[] = $state([]);
  activeKey: string = $state(HOME);
  sideTab: SideTab = $state("sessions");
  editing: Session | null = $state(null);
  shells: LocalShell[] = $state([]);
  recent: Recent[] = $state([]);
  prefs: Prefs = $state(
    loadJson(PREFS_KEY, { sidebar: true, toolbar: true, menubar: true, statusbar: true, sidebarWidth: 280 }),
  );

  textAsk: TextAsk | null = $state(null);
  confirmAsk: ConfirmAsk | null = $state(null);
  alertText = $state("");
  infoText = $state("");
  hostKeys: HostKeyPrompt[] = $state([]);
  tunnels: TunnelStatus[] = $state([]);
  showTunnels = $state(false);
  xserver: XServerStatus | null = $state(null);
  /** Open context menu (right click), if any. */
  ctx: { x: number; y: number; items: MenuItem[] } | null = $state(null);

  active = $derived(this.tabs.find((t) => t.key === this.activeKey) ?? null);
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

  newSession(folder = "", kind = "ssh") {
    this.editing = newSession(folder, kind);
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
  openSession(s: Session) {
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
    this.remember(session);
    this.connect(this.tabs[this.tabs.length - 1]);
  }

  /** Opens a local terminal with the given shell id (default shell when empty). */
  startLocalTerminal(shellId = "") {
    const sh = this.shells.find((x) => x.id === shellId) ?? this.shells[0];
    const s = newSession("", "shell");
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
    const s = newSession("", kind);
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
    tab.status = protocol(s.kind).remote ? `Connexion à ${describe(s)}…` : `Démarrage : ${describe(s)}…`;
    try {
      // size is corrected by the terminal right after attaching
      const connId = await this.withSecrets(s, (sec) =>
        api.termOpen({ session: $state.snapshot(s) as Session, cols: 120, rows: 32, ...sec }),
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
    if (tab.connId) await api.termClose(tab.connId).catch(() => {});
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

  async renameTab(tab: Tab) {
    const name = await this.askText("Renommer l'onglet", tab.title);
    if (name) tab.title = name;
  }

  nextTab(dir: 1 | -1) {
    const keys = [HOME, ...this.tabs.map((t) => t.key)];
    const i = keys.indexOf(this.activeKey);
    this.activeKey = keys[(i + dir + keys.length) % keys.length];
  }

  onConnectionClosed(connId: string) {
    const t = this.tabs.find((x) => x.connId === connId);
    if (t) {
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
