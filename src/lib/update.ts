// Update check against the GitHub releases of the project (when the user asks, and at
// start-up unless turned off in the settings). Installing is done by UpdateDialog.

export const RELEASES = "https://github.com/Kaguyu-M/orbiterm/releases";
const API = "https://api.github.com/repos/Kaguyu-M/orbiterm/releases/latest";

export interface UpdateInfo {
  latest: string;
  url: string;
  newer: boolean;
}

/** "0.7.10" > "0.7.9" (numeric comparison, an optional "v" prefix is ignored). */
export function isNewer(latest: string, current: string): boolean {
  const p = (v: string) => v.replace(/^v/, "").split(/[.-]/).map((x) => parseInt(x, 10) || 0);
  const [a, b] = [p(latest), p(current)];
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    if ((a[i] ?? 0) !== (b[i] ?? 0)) return (a[i] ?? 0) > (b[i] ?? 0);
  }
  return false;
}

export async function checkUpdate(current: string): Promise<UpdateInfo> {
  const ctl = new AbortController();
  const t = setTimeout(() => ctl.abort(), 10000);
  try {
    const r = await fetch(API, { headers: { Accept: "application/vnd.github+json" }, signal: ctl.signal });
    if (!r.ok) throw new Error(`GitHub a répondu ${r.status}`);
    const j = await r.json();
    const latest = String(j.tag_name ?? "").replace(/^v/, "");
    if (!latest) throw new Error("réponse inattendue");
    return { latest, url: j.html_url ?? RELEASES, newer: isNewer(latest, current) };
  } finally {
    clearTimeout(t);
  }
}
