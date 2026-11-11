// ZMODEM in the terminal (like MobaXterm): `sz file` on the server downloads into a folder
// chosen here, `rz` on the server sends files picked here. Built on zmodem.js (Apache-2.0).

import Zmodem from "zmodem.js/src/zmodem_browser.js";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "./api";

const CHUNK = 64 * 1024;

/** Automated GUI tests replace the native file dialogs: window.__orbitermZmodemPick = { files, dir }. */
const testPick = () => (window as unknown as { __orbitermZmodemPick?: { files?: string[]; dir?: string } }).__orbitermZmodemPick;

export function bytesToB64(u8: Uint8Array): string {
  let s = "";
  for (let i = 0; i < u8.length; i += 0x8000) s += String.fromCharCode(...u8.subarray(i, i + 0x8000));
  return btoa(s);
}

function b64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

function concat(parts: Uint8Array[]): Uint8Array {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let o = 0;
  for (const p of parts) {
    out.set(p, o);
    o += p.length;
  }
  return out;
}

const size = (n: number) =>
  n >= 1 << 30 ? `${(n / (1 << 30)).toFixed(1)} Go` : n >= 1 << 20 ? `${(n / (1 << 20)).toFixed(1)} Mo` : n >= 1024 ? `${(n / 1024).toFixed(0)} ko` : `${n} o`;
const message = (e: unknown) => (e instanceof Error ? e.message : typeof e === "string" ? e : JSON.stringify(e));

export interface ZmodemHost {
  /** bytes for the terminal display */
  write(bytes: Uint8Array): void;
  /** connection that carries the session */
  connId(): string;
}

export class ZmodemBridge {
  private sentry: any;
  private session: any = null;
  private queue: Promise<unknown> = Promise.resolve();

  constructor(private host: ZmodemHost) {
    this.sentry = this.newSentry();
  }

  private newSentry() {
    return new Zmodem.Sentry({
      to_terminal: (o: number[]) => this.host.write(new Uint8Array(o)),
      // keep the order of the bytes sent to the server
      sender: (o: number[]) => {
        const b = bytesToB64(new Uint8Array(o));
        this.queue = this.queue.then(() => api.termWriteBytes(this.host.connId(), b)).catch(() => {});
      },
      on_retract: () => this.info("transfert annulé par le serveur"),
      on_detect: (d: any) => this.run(d.confirm()),
    });
  }

  get active(): boolean {
    return this.session !== null;
  }

  consume(bytes: Uint8Array) {
    try {
      this.sentry.consume(bytes);
    } catch (e) {
      // lrzsz's sz may end without the final "OO": the files are complete, the bytes
      // are ordinary terminal output again
      const after = /after ZFIN/.test(message(e));
      if (!after) this.info(`erreur : ${message(e)}`);
      this.reset();
      this.host.write(bytes);
    }
  }

  /** Forgets the current session: everything goes to the terminal again. */
  private reset() {
    this.session = null;
    this.sentry = this.newSentry();
  }

  /** Ctrl+C during a transfer. */
  abort() {
    try {
      this.session?.abort();
    } catch {
      /* already over */
    }
    this.reset();
    this.info("transfert interrompu");
  }

  private info(msg: string) {
    this.host.write(new TextEncoder().encode(`\r\n\x1b[36m[ZMODEM] ${msg}\x1b[0m\r\n`));
  }

  private progress(name: string, done: number, total: number) {
    const pct = total ? Math.floor((done * 100) / total) : 100;
    this.host.write(new TextEncoder().encode(`\r\x1b[2K\x1b[36m[ZMODEM] ${name} : ${size(done)} / ${size(total)} (${pct} %)\x1b[0m`));
  }

  private async run(z: any) {
    this.session = z;
    try {
      if (z.type === "send") await this.upload(z);
      else await this.download(z);
    } catch (e) {
      this.info(`erreur : ${message(e)}`);
      try {
        z.abort();
      } catch {
        /* ignore */
      }
    } finally {
      this.session = null;
    }
  }

  /** The server runs `rz`: send local files. */
  private async upload(z: any) {
    const picked = testPick()?.files ?? (await open({ multiple: true, title: "Fichiers à envoyer au serveur (ZMODEM)" }));
    const paths = picked ? (Array.isArray(picked) ? picked : [picked]) : [];
    if (!paths.length) {
      this.info("aucun fichier choisi");
      await z.close();
      return;
    }
    const infos = await Promise.all(paths.map((p) => api.localFileInfo(p)));
    let remaining = infos.reduce((n, i) => n + i.size, 0);
    for (let k = 0; k < paths.length; k++) {
      const p = paths[k];
      const name = p.split(/[\\/]/).pop() ?? "fichier";
      const xfer = await z.send_offer({
        name,
        size: infos[k].size,
        mtime: new Date(infos[k].mtime * 1000),
        files_remaining: paths.length - k,
        bytes_remaining: remaining,
      });
      remaining -= infos[k].size;
      if (!xfer) {
        this.info(`${name} : refusé par le serveur (déjà présent ?)`);
        continue;
      }
      let off = xfer.get_offset?.() ?? 0;
      while (off < infos[k].size) {
        const data = b64ToBytes(await api.localReadChunk(p, off, CHUNK));
        if (!data.length) break;
        xfer.send(data);
        await this.queue; // do not read faster than the connection sends
        off += data.length;
        this.progress(name, off, infos[k].size);
      }
      await xfer.end();
      this.info(`${name} envoyé (${size(infos[k].size)})`);
    }
    await z.close();
  }

  /** The server runs `sz`: receive files into a folder chosen once per transfer. */
  private download(z: any): Promise<void> {
    let dir: string | null | undefined;
    // one chain for every file of the transfer: writes and messages stay in order
    let writes: Promise<unknown> = Promise.resolve();
    const done = new Promise<void>((res) => z.on("session_end", () => res()));
    z.on("offer", async (offer: any) => {
      const d = offer.get_details();
      const name = String(d.name).split(/[\\/]/).pop() || "fichier";
      if (dir === undefined) {
        const r = testPick()?.dir ?? (await open({ directory: true, title: "Dossier où enregistrer les fichiers reçus (ZMODEM)" }));
        dir = typeof r === "string" ? r : null;
      }
      if (!dir) {
        offer.skip();
        this.info(`${name} : ignoré (aucun dossier choisi)`);
        return;
      }
      const total = Number(d.size) || 0;
      let pending: Uint8Array[] = [];
      let pendingLen = 0;
      let got = 0;
      let path = "";
      const flush = (first: boolean) => {
        const chunk = concat(pending);
        pending = [];
        pendingLen = 0;
        const target = dir!;
        writes = writes.then(async () => {
          const p = await api.localWriteChunk(target, path ? path.split(/[\\/]/).pop()! : name, bytesToB64(chunk), first);
          path = p;
        });
      };
      flush(true); // creates the file (a new name if it already exists)
      try {
        await offer.accept({
          on_input: (payload: number[]) => {
            pending.push(new Uint8Array(payload));
            pendingLen += payload.length;
            got += payload.length;
            if (pendingLen >= 256 * 1024) flush(false);
            this.progress(name, got, total);
          },
        });
        flush(false);
        writes = writes.then(() => this.info(`reçu : ${path} (${size(got)})`));
      } catch (e) {
        this.info(`${name} : ${message(e)}`);
      }
    });
    z.start();
    // a failed local write is reported once the transfer is over
    return done
      .then(() => writes)
      .then(
        () => {},
        (e) => this.info(`écriture impossible : ${message(e)}`),
      );
  }
}
