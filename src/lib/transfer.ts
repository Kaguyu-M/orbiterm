// Progress of a file transfer: percentage, speed (smoothed) and time left, from the
// (done, total) byte counts the backend reports about ten times per second.
import { formatSize } from "./api";

export interface Meter {
  done: number;
  total: number;
  /** ms (performance.now) of the first report, of the last speed sample */
  started: number;
  sampleAt: number;
  sampleDone: number;
  /** bytes/s, smoothed; 0 until known */
  rate: number;
}

export function newMeter(now: number): Meter {
  return { done: 0, total: 0, started: now, sampleAt: now, sampleDone: 0, rate: 0 };
}

/** Speed samples over at least this long: the reports are bursty (one per network chunk). */
const SAMPLE_MS = 400;

export function update(m: Meter, done: number, total: number, now: number) {
  m.done = done;
  m.total = total;
  const dt = now - m.sampleAt;
  if (dt < SAMPLE_MS) return;
  const inst = ((done - m.sampleDone) * 1000) / dt;
  // exponential smoothing: steady figure, yet follows a real change of speed in a few seconds
  m.rate = m.rate > 0 ? m.rate * 0.7 + inst * 0.3 : inst;
  m.sampleAt = now;
  m.sampleDone = done;
}

export function percent(m: Meter): number {
  return m.total > 0 ? Math.min(100, Math.floor((m.done * 100) / m.total)) : 0;
}

/** "45 s", "3 min 05 s", "1 h 02 min". */
export function formatDuration(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  if (s < 60) return `${s} s`;
  if (s < 3600) return `${Math.floor(s / 60)} min ${String(s % 60).padStart(2, "0")} s`;
  return `${Math.floor(s / 3600)} h ${String(Math.floor((s % 3600) / 60)).padStart(2, "0")} min`;
}

export function formatRate(bytesPerSecond: number): string {
  return `${formatSize(Math.round(bytesPerSecond))}/s`;
}

/** Line under a running transfer: "120 Mo / 300 Mo · 12 Mo/s · reste 15 s". */
export function runningText(m: Meter): string {
  const parts = [m.total ? `${formatSize(m.done)} / ${formatSize(m.total)}` : formatSize(m.done)];
  if (m.rate > 0) {
    parts.push(formatRate(m.rate));
    if (m.total > m.done) parts.push(`reste ${formatDuration(Math.max(1, (m.total - m.done) / m.rate))}`);
  } else parts.push("calcul de la vitesse…");
  return parts.join(" · ");
}

/** Line of a finished transfer: "300 Mo en 25 s (12 Mo/s)". */
export function doneText(m: Meter, now: number): string {
  const secs = (now - m.started) / 1000;
  const bytes = m.total || m.done;
  return secs >= 1 ? `${formatSize(bytes)} en ${formatDuration(secs)} (${formatRate(bytes / secs)})` : formatSize(bytes);
}
