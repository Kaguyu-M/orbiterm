// Typed wrappers around the Rust commands (see src-tauri/src/commands.rs).
import { invoke } from "@tauri-apps/api/core";

export type AuthMethod = "password" | "key";

/** One sample of the remote system information (see src-tauri/src/monitor.rs). */
export interface RemoteStats {
  supported: boolean;
  os: string;
  host: string;
  cpu_total: number;
  cpu_idle: number;
  cpus: number;
  load: string;
  mem_total: number;
  mem_avail: number;
  swap_total: number;
  swap_free: number;
  disk_total: number;
  disk_used: number;
  net_rx: number;
  net_tx: number;
  uptime: number;
  users: number;
}

export interface TerminalSettings {
  font_family: string;
  font_size: number;
  scrollback: number;
  theme: string;
}

export interface Session {
  id: string;
  name: string;
  folder: string;
  kind: string;
  host: string;
  port: number;
  username: string;
  auth: AuthMethod;
  key_path: string;
  remember_password: boolean;
  sftp_enabled: boolean;
  terminal: TerminalSettings;
  /** Protocol-specific options (string values). */
  extra: Record<string, string>;
  /** SSH port forwardings started with the session. */
  forwards: Forward[];
}

export type ForwardKind = "local" | "remote" | "dynamic";

export interface Forward {
  kind: ForwardKind;
  bind_host: string;
  bind_port: number;
  dest_host: string;
  dest_port: number;
}

export interface Tunnel {
  id: string;
  name: string;
  session_id: string;
  forward: Forward;
  autostart: boolean;
}

export interface TunnelStatus extends Tunnel {
  running: boolean;
  port: number;
}

export interface ActiveForward {
  forward: Forward;
  port: number;
}

export interface XServerStatus {
  kind: string;
  path: string;
  running: boolean;
  display: string;
  owned: boolean;
}

export function newForward(kind: ForwardKind = "local"): Forward {
  return { kind, bind_host: "", bind_port: kind === "dynamic" ? 1080 : 8080, dest_host: kind === "remote" ? "127.0.0.1" : "", dest_port: 80 };
}

export function forwardLabel(f: Forward, port = f.bind_port): string {
  const bind = f.bind_host || (f.kind === "remote" ? "serveur" : "127.0.0.1");
  if (f.kind === "dynamic") return `SOCKS ${bind}:${port}`;
  return `${f.kind === "local" ? "Local" : "Distant"} ${bind}:${port} → ${f.dest_host}:${f.dest_port}`;
}

export interface Folder {
  id: string;
  name: string;
  parent: string;
}

export interface Store {
  version: number;
  folders: Folder[];
  sessions: Session[];
  tunnels: Tunnel[];
  macros: Macro[];
}

export interface MacroStep {
  data: string;
  delay_ms: number;
}

export interface Macro {
  id: string;
  name: string;
  steps: MacroStep[];
}

export interface ImportSource {
  id: string;
  label: string;
  path: string;
  count: number;
}

export interface ImportReport {
  source: string;
  added: number;
  duplicates: number;
  folders: number;
  skipped: string[];
}

export interface Entry {
  name: string;
  path: string;
  kind: "dir" | "file" | "link" | "other";
  size: number;
  mtime: number;
  perms: string;
}

export interface HostKeyPrompt {
  request_id: string;
  host: string;
  port: number;
  algorithm: string;
  fingerprint: string;
  status: "unknown" | "changed" | "known";
}

/** Error shape produced by the backend (AppError). */
export interface ApiError {
  kind: string;
  message: string;
}

export function asApiError(e: unknown): ApiError {
  if (e && typeof e === "object" && "kind" in e && "message" in e) return e as ApiError;
  return { kind: "unknown", message: String(e) };
}

export function newSession(folder = "", kind = "ssh"): Session {
  return {
    id: "",
    name: "",
    folder,
    kind,
    host: "",
    port: protocol(kind).port,
    username: "",
    auth: "password",
    key_path: "",
    remember_password: true,
    sftp_enabled: true,
    terminal: {
      font_family: "Cascadia Mono, Consolas, 'DejaVu Sans Mono', monospace",
      font_size: 14,
      scrollback: 10000,
      theme: "dark",
    },
    extra: {},
    forwards: [],
  };
}

export interface LocalShell {
  id: string;
  label: string;
  program: string;
  args: string[];
}

export interface Protocol {
  kind: string;
  label: string;
  icon: string;
  port: number;
  /** false = shown but not available yet */
  ready: boolean;
  /** needs a remote host */
  remote: boolean;
}

/** Session types, in the order of the session dialog. */
export const PROTOCOLS: Protocol[] = [
  { kind: "ssh", label: "SSH", icon: "ssh", port: 22, ready: true, remote: true },
  { kind: "telnet", label: "Telnet", icon: "telnet", port: 23, ready: true, remote: true },
  { kind: "rlogin", label: "Rlogin", icon: "rlogin", port: 513, ready: true, remote: true },
  { kind: "rdp", label: "RDP", icon: "rdp", port: 3389, ready: true, remote: true },
  { kind: "vnc", label: "VNC", icon: "vnc", port: 5900, ready: true, remote: true },
  { kind: "ftp", label: "FTP", icon: "ftp", port: 21, ready: true, remote: true },
  { kind: "sftp", label: "SFTP", icon: "sftp", port: 22, ready: true, remote: true },
  { kind: "serial", label: "Série", icon: "serial", port: 0, ready: true, remote: false },
  { kind: "shell", label: "Shell", icon: "shell", port: 0, ready: true, remote: false },
  { kind: "browser", label: "Navigateur", icon: "browser", port: 443, ready: true, remote: true },
  { kind: "mosh", label: "Mosh", icon: "mosh", port: 22, ready: true, remote: true },
  { kind: "wsl", label: "WSL", icon: "wsl", port: 0, ready: true, remote: false },
];

/** How a tab of this kind is displayed. */
export type TabView = "terminal" | "files" | "vnc" | "rdp" | "external" | "browser" | "editor" | "diff";

/** Text file opened in the editor: `text` uses "
", written back with the same format. */
export interface TextFile {
  text: string;
  encoding: "utf8" | "utf8-bom" | "utf16le" | "latin1";
  crlf: boolean;
}

/** Pseudo-protocol of the editor tabs (not a session type of the dialog). */
export const EDITOR: Protocol = { kind: "editor", label: "Éditeur", icon: "edit", port: 0, ready: true, remote: false };
export const DIFF: Protocol = { kind: "diff", label: "Comparaison", icon: "split", port: 0, ready: true, remote: false };
export const IS_WINDOWS = typeof navigator !== "undefined" && /windows/i.test(navigator.userAgent);
export function viewOf(s: Pick<Session, "kind" | "extra">): TabView {
  const kind = s.kind;
  if (kind === "sftp" || kind === "ftp") return "files";
  if (kind === "vnc") return "vnc";
  if (kind === "browser") return "browser";
  if (kind === "editor") return "editor";
  if (kind === "diff") return "diff";
  if (kind === "rdp") {
    // embedded client by default; optional external client: mstsc window (Windows) / FreeRDP (elsewhere)
    if (s.extra?.rdp_client !== "mstsc") return "rdp";
    return IS_WINDOWS ? "external" : "terminal";
  }
  return "terminal";
}

export function protocol(kind: string): Protocol {
  if (kind === "editor") return EDITOR;
  if (kind === "diff") return DIFF;
  return PROTOCOLS.find((p) => p.kind === kind) ?? PROTOCOLS[0];
}

/** Short description shown in tooltips / status bar. */
export function describe(s: Session): string {
  const p = protocol(s.kind);
  if (s.kind === "shell") return s.extra?.program || s.name || "Terminal local";
  if (s.kind === "wsl") return `WSL ${s.extra?.distro || "(par défaut)"}`;
  if (s.kind === "serial") return `Série ${s.extra?.serial_port || s.host} @ ${s.extra?.baud || "115200"}`;
  if (s.kind === "browser") return s.host;
  if (s.kind === "diff") return s.extra?.left && s.extra?.right ? `${s.extra.left}  ↔  ${s.extra.right}` : "Comparaison";
  if (s.kind === "editor") return s.extra?.path ? `${s.extra.source ? s.extra.source + " : " : ""}${s.extra.path}` : "Nouveau fichier";
  return `${p.label} ${s.username ? s.username + "@" : ""}${s.host}${s.port && s.port !== p.port ? ":" + s.port : ""}`;
}

export const api = {
  sessionsLoad: () => invoke<Store>("sessions_load"),
  sessionSave: (session: Session) => invoke<Session>("session_save", { session }),
  sessionDelete: (id: string) => invoke<void>("session_delete", { id }),
  folderSave: (folder: Folder) => invoke<Folder>("folder_save", { folder }),
  folderDelete: (id: string) => invoke<void>("folder_delete", { id }),
  passwordForget: (id: string) => invoke<void>("password_forget", { id }),
  /** System information of the remote host of a tab (over SSH). */
  monitorSample: (connId: string) => invoke<RemoteStats>("monitor_sample", { connId }),
  /** Copies the remembered passwords of a saved session to another one (keyring only). */
  passwordCopy: (from: string, to: string) => invoke<boolean>("password_copy", { from, to }),
  importSources: () => invoke<ImportSource[]>("import_sources"),
  importRun: (source: string, path?: string) => invoke<ImportReport>("import_run", { source, path }),
  macroSave: (m: Macro) => invoke<Macro>("macro_save", { m }),
  macroDelete: (id: string) => invoke<void>("macro_delete", { id }),
  configPath: () => invoke<string>("config_path"),

  termOpen: (args: {
    session: Session;
    cols: number;
    rows: number;
    password?: string;
    passphrase?: string;
    jump_password?: string;
    remember?: boolean;
  }) => invoke<string>("term_open", { args }),
  hostkeyAnswer: (requestId: string, accept: boolean) =>
    invoke<void>("hostkey_answer", { requestId, accept }),
  termWrite: (connId: string, data: string) => invoke<void>("term_write", { connId, data }),
  termWriteBytes: (connId: string, data: string) => invoke<void>("term_write_bytes", { connId, data }),
  localFileInfo: (path: string) => invoke<{ size: number; mtime: number }>("local_file_info", { path }),
  localReadChunk: (path: string, offset: number, len: number) => invoke<string>("local_read_chunk", { path, offset, len }),
  localWriteChunk: (dir: string, name: string, data: string, create: boolean) =>
    invoke<string>("local_write_chunk", { dir, name, data, create }),
  termLog: (connId: string, text: string) => invoke<void>("term_log", { connId, text }),
  termResize: (connId: string, cols: number, rows: number) =>
    invoke<void>("term_resize", { connId, cols, rows }),
  termClose: (connId: string) => invoke<void>("term_close", { connId }),
  localShells: () => invoke<LocalShell[]>("local_shells"),
  serialPorts: () => invoke<string[]>("serial_ports"),
  termForwards: (connId: string) => invoke<ActiveForward[]>("term_forwards", { connId }),

  tunnelsList: () => invoke<TunnelStatus[]>("tunnels_list"),
  tunnelSave: (tunnel: Tunnel) => invoke<Tunnel>("tunnel_save", { tunnel }),
  tunnelDelete: (id: string) => invoke<void>("tunnel_delete", { id }),
  tunnelStart: (args: { id: string; password?: string; passphrase?: string; jump_password?: string; remember?: boolean }) =>
    invoke<number>("tunnel_start", { args }),
  tunnelStop: (id: string) => invoke<void>("tunnel_stop", { id }),

  xserverStatus: () => invoke<XServerStatus>("xserver_status"),
  xserverStart: () => invoke<XServerStatus>("xserver_start"),
  xserverStop: () => invoke<XServerStatus>("xserver_stop"),

  vncUrl: (connId: string) => invoke<string>("vnc_url", { connId }),
  rdpUrl: (connId: string) => invoke<{ url: string; width: number; height: number }>("rdp_url", { connId }),
  vncPasswordGet: (sessionId: string) => invoke<string | null>("vnc_password_get", { sessionId }),
  vncPasswordSet: (sessionId: string, password: string | null) => invoke<void>("vnc_password_set", { sessionId, password }),
  externalRunning: (connId: string) => invoke<boolean>("external_running", { connId }),
  browserOpen: (label: string, url: string, x: number, y: number, w: number, h: number) =>
    invoke<string>("browser_open", { label, url, x, y, w, h }),
  browserBounds: (label: string, x: number, y: number, w: number, h: number, visible: boolean) =>
    invoke<void>("browser_bounds", { label, x, y, w, h, visible }),
  browserNavigate: (label: string, url: string) => invoke<string>("browser_navigate", { label, url }),
  browserAction: (label: string, action: "back" | "forward" | "reload" | "stop") => invoke<void>("browser_action", { label, action }),
  browserClose: (label: string) => invoke<void>("browser_close", { label }),

  configExport: (path: string, prefs: unknown) => invoke<void>("config_export", { path, prefs }),
  configImport: (path: string) =>
    invoke<{ stats: { sessions: number; folders: number; macros: number; tunnels: number; skipped: number }; prefs: Record<string, unknown> | null }>(
      "config_import",
      { path },
    ),
  dirCompare: (left: string, right: string) =>
    invoke<{
      entries: Array<{ path: string; dir: boolean; status: "same" | "different" | "left_only" | "right_only" | "kind_mismatch"; left_size: number | null; right_size: number | null; left_mtime: number | null; right_mtime: number | null }>;
      truncated: boolean;
    }>("dir_compare", { left, right }),
  textRead: (connId: string, path: string) => invoke<TextFile>("text_read", { connId, path }),
  textWrite: (connId: string, path: string, file: TextFile) => invoke<void>("text_write", { connId, path, file }),
  sshCwd: (connId: string) => invoke<string | null>("ssh_cwd", { connId }),
  sftpHome: (connId: string) => invoke<string>("sftp_home", { connId }),
  sftpMode: (connId: string) => invoke<"sftp" | "scp" | "ftp">("sftp_mode", { connId }),
  sftpList: (connId: string, path: string) => invoke<Entry[]>("sftp_list", { connId, path }),
  sftpMkdir: (connId: string, path: string) => invoke<void>("sftp_mkdir", { connId, path }),
  sftpRename: (connId: string, from: string, to: string) =>
    invoke<void>("sftp_rename", { connId, from, to }),
  sftpRemove: (connId: string, path: string) => invoke<void>("sftp_remove", { connId, path }),
  sftpDownload: (connId: string, remote: string, localDir: string, transferId: string) =>
    invoke<string>("sftp_download", { connId, remote, localDir, transferId }),
  sftpUpload: (connId: string, local: string, remoteDir: string, transferId: string) =>
    invoke<string>("sftp_upload", { connId, local, remoteDir, transferId }),

  serversStatus: () => invoke<ServerStatus[]>("servers_status"),
  serverSave: (config: ServerConfig, password?: string) => invoke<void>("server_save", { config, password }),
  serverStart: (kind: ServerKind) => invoke<number>("server_start", { kind }),
  serverStop: (kind: ServerKind) => invoke<void>("server_stop", { kind }),
  serverClearLog: (kind: ServerKind) => invoke<void>("server_clear_log", { kind }),
  scanStart: (id: string, targets: string, ports: string, timeoutMs: number, concurrency: number) =>
    invoke<number>("scan_start", { id, targets, ports, timeoutMs, concurrency }),
  scanState: (id: string) => invoke<ScanState | null>("scan_state", { id }),
  scanCancel: (id: string) => invoke<void>("scan_cancel", { id }),
  wolSend: (mac: string, broadcast: string, port: number) => invoke<void>("wol_send", { mac, broadcast, port }),
  openPorts: () => invoke<SocketInfo[]>("open_ports"),
  keyGenerate: (kind: string, comment: string) => invoke<KeyInfo>("key_generate", { kind, comment }),
  keyLoad: (path: string, passphrase?: string) => invoke<KeyInfo>("key_load", { path, passphrase }),
  keySetComment: (comment: string) => invoke<KeyInfo>("key_set_comment", { comment }),
  keySavePrivate: (path: string, format: "openssh" | "ppk", passphrase: string) => invoke<void>("key_save_private", { path, format, passphrase }),
  keySavePublic: (path: string) => invoke<void>("key_save_public", { path }),
  keyForget: () => invoke<void>("key_forget"),
  agentState: () => invoke<AgentState>("agent_state"),
  agentAdd: (path: string, passphrase?: string) => invoke<AgentKey>("agent_add", { path, passphrase }),
  agentRemove: (fingerprint: string) => invoke<void>("agent_remove", { fingerprint }),
};

/** Key of the internal agent (internal) or of the system agent. */
export interface AgentKey {
  fingerprint: string;
  algorithm: string;
  comment: string;
  source: string;
  internal: boolean;
}
export interface AgentState {
  keys: AgentKey[];
  /** running system agent ("" when none) */
  system: string;
}

/** Key held by the SSH key generator (the private part stays in the backend). */
export interface KeyInfo {
  algorithm: string;
  bits: number;
  comment: string;
  public_openssh: string;
  fingerprint: string;
  randomart: string;
}

export type ServerKind = "http" | "tftp" | "ftp" | "ssh" | "telnet" | "iperf" | "vnc";
export interface ServerConfig {
  kind: ServerKind;
  port: number;
  root: string;
  local_only: boolean;
  writable: boolean;
  anonymous: boolean;
  user: string;
  listing: boolean;
  shell: string;
  autostart: boolean;
  /** SSH: authorized_keys lines */
  authorized_keys: string;
}
export interface ServerStatus {
  config: ServerConfig;
  running: boolean;
  port: number;
  started: number;
  clients: number;
  total: number;
  has_password: boolean;
  addresses: string[];
  root: string;
  log: { t: number; msg: string }[];
}
export interface ScanHit {
  host: string;
  ip: string;
  port: number;
  service: string;
}
export interface ScanState {
  total: number;
  done: number;
  hits: ScanHit[];
  finished: boolean;
  cancelled: boolean;
  hosts: number;
}
export interface SocketInfo {
  proto: string;
  local: string;
  local_port: number;
  remote: string;
  state: string;
  pid: number;
  process: string;
}

export function remoteJoin(dir: string, name: string): string {
  return dir.endsWith("/") ? dir + name : `${dir}/${name}`;
}

export function remoteParent(path: string): string {
  const t = path.replace(/\/+$/, "");
  const i = t.lastIndexOf("/");
  return i <= 0 ? "/" : t.slice(0, i);
}

export function formatSize(n: number): string {
  if (n < 1024) return `${n} o`;
  const u = ["Ko", "Mo", "Go", "To"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < u.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v < 10 ? 1 : 0)} ${u[i]}`;
}
