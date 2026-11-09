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

export function newSession(folder = ""): Session {
  return {
    id: "",
    name: "",
    folder,
    kind: "ssh",
    host: "",
    port: 22,
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
  };
}

export const api = {
  sessionsLoad: () => invoke<Store>("sessions_load"),
  sessionSave: (session: Session) => invoke<Session>("session_save", { session }),
  sessionDelete: (id: string) => invoke<void>("session_delete", { id }),
  folderSave: (folder: Folder) => invoke<Folder>("folder_save", { folder }),
  folderDelete: (id: string) => invoke<void>("folder_delete", { id }),
  passwordForget: (id: string) => invoke<void>("password_forget", { id }),

  sshConnect: (args: {
    session_id: string;
    cols: number;
    rows: number;
    password?: string;
    passphrase?: string;
    remember?: boolean;
  }) => invoke<string>("ssh_connect", { args }),
  hostkeyAnswer: (requestId: string, accept: boolean) =>
    invoke<void>("hostkey_answer", { requestId, accept }),
  sshWrite: (connId: string, data: string) => invoke<void>("ssh_write", { connId, data }),
  sshResize: (connId: string, cols: number, rows: number) =>
    invoke<void>("ssh_resize", { connId, cols, rows }),
  sshDisconnect: (connId: string) => invoke<void>("ssh_disconnect", { connId }),

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
