// Routes backend terminal events to the right terminal component.
//
// One global listener is installed at startup. Output that arrives before the
// terminal component is attached (e.g. the server banner) is buffered, so
// nothing is lost.
import { listen } from "@tauri-apps/api/event";

type Sink = { data: (bytes: Uint8Array) => void; closed: (reason: string) => void };

const sinks = new Map<string, Sink>();
const pending = new Map<string, Uint8Array[]>();
const pendingClose = new Map<string, string>();
let started = false;

function b64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

export async function startBus() {
  if (started) return;
  started = true;
  await listen<{ conn_id: string; data: string }>("term-data", (e) => {
    const bytes = b64ToBytes(e.payload.data);
    const sink = sinks.get(e.payload.conn_id);
    if (sink) sink.data(bytes);
    else {
      const list = pending.get(e.payload.conn_id) ?? [];
      list.push(bytes);
      pending.set(e.payload.conn_id, list);
    }
  });
  await listen<{ conn_id: string; reason: string }>("term-closed", (e) => {
    const sink = sinks.get(e.payload.conn_id);
    if (sink) sink.closed(e.payload.reason);
    else pendingClose.set(e.payload.conn_id, e.payload.reason);
    for (const cb of closeWatchers) cb(e.payload.conn_id, e.payload.reason);
  });
}

const closeWatchers = new Set<(connId: string, reason: string) => void>();

/** Called by the page to update tab state when a connection ends. */
export function onConnectionClosed(cb: (connId: string, reason: string) => void) {
  closeWatchers.add(cb);
  return () => closeWatchers.delete(cb);
}

export function attach(connId: string, sink: Sink) {
  sinks.set(connId, sink);
  for (const chunk of pending.get(connId) ?? []) sink.data(chunk);
  pending.delete(connId);
  const r = pendingClose.get(connId);
  if (r !== undefined) {
    pendingClose.delete(connId);
    sink.closed(r);
  }
  return () => {
    sinks.delete(connId);
  };
}
