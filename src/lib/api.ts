// Typed wrappers around the Rust commands (see src-tauri/src/commands.rs).
import { invoke } from "@tauri-apps/api/core";

export type AuthMethod = "password" | "key";

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
  { kind: "rdp", label: "RDP", icon: "rdp", port: 3389, ready: false, remote: true },
  { kind: "vnc", label: "VNC", icon: "vnc", port: 5900, ready: false, remote: true },
  { kind: "ftp", label: "FTP", icon: "ftp", port: 21, ready: false, remote: true },
  { kind: "sftp", label: "SFTP", icon: "sftp", port: 22, ready: false, remote: true },
  { kind: "serial", label: "Série", icon: "serial", port: 0, ready: true, remote: false },
  { kind: "shell", label: "Shell", icon: "shell", port: 0, ready: true, remote: false },
  { kind: "browser", label: "Navigateur", icon: "browser", port: 443, ready: false, remote: true },
  { kind: "mosh", label: "Mosh", icon: "mosh", port: 22, ready: true, remote: true },
  { kind: "wsl", label: "WSL", icon: "wsl", port: 0, ready: true, remote: false },
];

export function protocol(kind: string): Protocol {
  return PROTOCOLS.find((p) => p.kind === kind) ?? PROTOCOLS[0];
}

/** Short description shown in tooltips / status bar. */
export function describe(s: Session): string {
  const p = protocol(s.kind);
  if (s.kind === "shell") return s.extra?.program || s.name || "Terminal local";
  if (s.kind === "wsl") return `WSL ${s.extra?.distro || "(par défaut)"}`;
  if (s.kind === "serial") return `Série ${s.extra?.serial_port || s.host} @ ${s.extra?.baud || "115200"}`;
  return `${p.label} ${s.username ? s.username + "@" : ""}${s.host}${s.port && s.port !== p.port ? ":" + s.port : ""}`;
}

export const api = {
  sessionsLoad: () => invoke<Store>("sessions_load"),
  sessionSave: (session: Session) => invoke<Session>("session_save", { session }),
  sessionDelete: (id: string) => invoke<void>("session_delete", { id }),
  folderSave: (folder: Folder) => invoke<Folder>("folder_save", { folder }),
  folderDelete: (id: string) => invoke<void>("folder_delete", { id }),
  passwordForget: (id: string) => invoke<void>("password_forget", { id }),

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

  sftpHome: (connId: string) => invoke<string>("sftp_home", { connId }),
  sftpList: (connId: string, path: string) => invoke<Entry[]>("sftp_list", { connId, path }),
  sftpMkdir: (connId: string, path: string) => invoke<void>("sftp_mkdir", { connId, path }),
  sftpRename: (connId: string, from: string, to: string) =>
    invoke<void>("sftp_rename", { connId, from, to }),
  sftpRemove: (connId: string, path: string) => invoke<void>("sftp_remove", { connId, path }),
  sftpDownload: (connId: string, remote: string, localDir: string, transferId: string) =>
    invoke<string>("sftp_download", { connId, remote, localDir, transferId }),
  sftpUpload: (connId: string, local: string, remoteDir: string, transferId: string) =>
    invoke<string>("sftp_upload", { connId, local, remoteDir, transferId }),
};

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
