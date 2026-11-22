// Keywords in colour in the terminals (like the "syntax highlighting" of the original):
// errors in red, warnings in amber, successes in green. Lines are coloured once complete
// (the cursor has left them), with xterm decorations: the data itself is never changed.
import type { IMarker, Terminal } from "@xterm/xterm";

/**
 * Follows the lines of the normal buffer that were completed since the last call.
 * A marker keeps the position even when old lines leave the scrollback.
 */
export class LineTracker {
  private marker: IMarker | undefined;
  constructor(
    private term: Terminal,
    /** first call: from the top of the buffer (log) or only the last screens (colours) */
    private fromStart: boolean,
  ) {}

  /** [start, end) of the new complete lines (with the cursor line when `withCursorLine`), or null. */
  take(withCursorLine = false): [number, number] | null {
    const buf = this.term.buffer.active;
    if (buf.type !== "normal") return null; // full-screen programs (vim, top…)
    const cur = buf.baseY + buf.cursorY;
    const end = withCursorLine ? cur + 1 : cur;
    let start: number;
    if (this.marker && !this.marker.isDisposed) start = this.marker.line;
    else if (this.marker) start = 0; // trimmed or cleared meanwhile
    else start = this.fromStart ? 0 : Math.max(0, cur - this.term.rows * 3);
    this.marker?.dispose();
    this.marker = this.term.registerMarker(0);
    if (withCursorLine && this.marker) {
      // the cursor line is now logged: next time, start after it
      const next = this.term.registerMarker(1);
      if (next) {
        this.marker.dispose();
        this.marker = next;
      }
    }
    return start < end ? [start, end] : null;
  }
}

/** Text of the lines [start, end): wrapped lines are joined back. */
export function linesText(term: Terminal, [start, end]: [number, number]): string {
  const buf = term.buffer.active;
  let out = "";
  for (let y = start; y < end; y++) {
    const line = buf.getLine(y);
    if (!line) continue;
    if (y > start && !line.isWrapped) out += "\n";
    out += line.translateToString(!buf.getLine(y + 1)?.isWrapped);
  }
  return out;
}

const B = "(?<![\\p{L}\\p{N}_])";
const E = "(?![\\p{L}\\p{N}_])";
const word = (words: string) => new RegExp(`${B}(?:${words})${E}`, "giu");

type Kind = "err" | "warn" | "ok" | "net" | "time" | "url" | "user" | "root" | "host" | "path";

const IPV4_PART = "(?:25[0-5]|2[0-4]\\d|1?\\d?\\d)";
const H = "[0-9a-f]{1,4}";

/** In order of priority: a piece of text already coloured by a rule is left to it. */
const RULES: Array<{ re: RegExp; kind: Kind }> = [
  { re: /\b(?:https?|ftp|sftp|ssh):\/\/[^\s"'<>`]+/giu, kind: "url" },
  {
    re: word(
      "errors?|erreurs?|fail(?:ed|ure|s)?|échecs?|échoué|fatal|denied|refusée?s?|refused|critical|critique|panic|exception|abort(?:ed)?|segmentation fault" +
        "|inactive|inactif|dead|stopped|arrêtée?|offline|hors ligne|disconnected|déconnectée?|unreachable|injoignable|timed out|timeout|killed" +
        "|not found|introuvable|no such file or directory|permission denied|invalid|invalide",
    ),
    kind: "err",
  },
  {
    re: word("warnings?|warn|attention|avertissements?|deprecated|obsolète|disabled|désactivée?|skipped|ignorée?|retrying|pending|en attente|degraded|dégradée?"),
    kind: "warn",
  },
  {
    re: word(
      "ok|success(?:ful(?:ly)?)?|succès|réussie?s?|passed|completed|terminée?" +
        "|active|actif|running|en cours|enabled|activée?|online|en ligne|connected|connectée?|established|listen(?:ing)?|accepted|acceptée?",
    ),
    kind: "ok",
  },
  // network: IPv4 (with /mask or :port), IPv6, MAC addresses
  { re: new RegExp(`(?<![\\w.])(?:${IPV4_PART}\\.){3}${IPV4_PART}(?:\\/\\d{1,2}|:\\d{1,5})?(?![\\w.])`, "g"), kind: "net" },
  {
    re: new RegExp(`(?<![\\w:])(?=\\S{3})(?:(?:${H}:){7}${H}|(?:${H}(?::${H})*)?::(?:${H}(?::${H})*)?)(?:\\/\\d{1,3})?(?![\\w:])`, "gi"),
    kind: "net",
  },
  { re: /(?<![\w:-])(?:[0-9a-f]{2}[:-]){5}[0-9a-f]{2}(?![\w:-])/gi, kind: "net" },
  // dates and times
  { re: /(?<![\w-])\d{4}-\d{2}-\d{2}(?:[T ]\d{2}:\d{2}(?::\d{2}(?:[.,]\d+)?)?(?:Z|[+-]\d{2}:?\d{2})?)?(?![\w-])/g, kind: "time" },
  { re: /(?<![\w/])\d{1,2}\/\d{1,2}\/\d{2,4}(?![\w/])/g, kind: "time" },
  { re: /(?<![\w:.])\d{1,2}:\d{2}(?::\d{2}(?:[.,]\d+)?)?(?![\w:])/g, kind: "time" },
];

/** Shell prompt without colours of its own: "user@host:path$" or "[user@host path]#". */
const PROMPT = /^(?:\([^)]*\) )?(\[)?([\w.-]+)(@)([\w.-]+)(:| )([^\s#$\]]*)(\]?)([#$%>])(?= |$)/u;

const DARK: Record<Kind, string> = {
  err: "#ff6b5b", warn: "#f0b45e", ok: "#9cc96b", net: "#5fd0e0", time: "#c792ea", url: "#7aa9ff",
  user: "#9cc96b", root: "#ff6b5b", host: "#5fd0e0", path: "#7aa9ff",
};
const LIGHT: Record<Kind, string> = {
  err: "#b3261e", warn: "#9a6200", ok: "#3f6f1f", net: "#00798c", time: "#8e3a9d", url: "#0067c0",
  user: "#3f6f1f", root: "#b3261e", host: "#00798c", path: "#0067c0",
};
const LIGHT_THEMES = new Set(["light", "creme", "graphite-light"]);

/** Max decorations created per pass (very fast output: the rest is skipped, not queued). */
const BUDGET = 400;

type Line = NonNullable<ReturnType<Terminal["buffer"]["active"]["getLine"]>>;

/** Pieces [index, length, kind] of a line of text to colour (no overlap). */
export function matches(text: string): Array<[number, number, Kind]> {
  const out: Array<[number, number, Kind]> = [];
  const free = (a: number, b: number) => out.every(([i, n]) => b <= i || a >= i + n);
  const p = PROMPT.exec(text);
  if (p) {
    let i = 0;
    const kinds: Array<Kind | null> = [null, null, p[2] === "root" ? "root" : "user", p[2] === "root" ? "root" : "user", "host", null, "path", null, null];
    for (let g = 1; g < p.length; g++) {
      const part = p[g] ?? "";
      const kind = kinds[g];
      if (kind && part) out.push([i, part.length, kind]);
      i += part.length;
    }
  }
  for (const { re, kind } of RULES) {
    re.lastIndex = 0;
    for (const m of text.matchAll(re)) {
      const i = m.index ?? 0;
      if (free(i, i + m[0].length)) out.push([i, m[0].length, kind]);
    }
  }
  return out;
}

function decorate(term: Terminal, line: Line, offset: number, pieces: Array<[number, number, Kind]>, colors: Record<Kind, string>, budget: number, keep?: Array<{ dispose(): void }>) {
  let used = 0;
  for (const [index, length, kind] of pieces) {
    if (used >= budget) break;
    used++;
    const marker = term.registerMarker(offset);
    if (!marker) continue;
    const deco = term.registerDecoration({ marker, x: cellIndex(line, index), width: length, foregroundColor: colors[kind], layer: "top" });
    if (!deco) {
      marker.dispose();
      continue;
    }
    deco.onDispose(() => marker.dispose());
    keep?.push(deco);
  }
  return used;
}

/** The prompt must have no colour of its own (a coloured PS1 is left as the server drew it). */
function plainPrompt(line: Line, pieces: Array<[number, number, Kind]>) {
  const first = pieces.find(([, , k]) => k === "user" || k === "root");
  if (!first) return pieces;
  const cell = line.getCell(cellIndex(line, first[0]));
  if (cell && cell.isFgDefault()) return pieces;
  return pieces.filter(([, , k]) => k !== "user" && k !== "root" && k !== "host" && k !== "path");
}

/** Colours the keywords of the lines [start, end) of the normal buffer. */
export function highlightLines(term: Terminal, [start, end]: [number, number], theme: string) {
  const buf = term.buffer.active;
  const cursorLine = buf.baseY + buf.cursorY;
  const colors = LIGHT_THEMES.has(theme) ? LIGHT : DARK;
  let budget = BUDGET;
  for (let y = Math.max(start, end - term.rows * 3); y < end && budget > 0; y++) {
    const line = buf.getLine(y);
    if (!line) continue;
    const text = line.translateToString(true);
    if (!text.trim()) continue;
    budget -= decorate(term, line, y - cursorLine, plainPrompt(line, matches(text)), colors, budget);
  }
}

/**
 * The line being typed changes all the time: only its prompt is coloured, with decorations
 * replaced at each pass (the line is coloured for good once complete).
 */
export class CursorPrompt {
  private decos: Array<{ dispose(): void }> = [];
  update(term: Terminal, theme: string, enabled: boolean) {
    for (const d of this.decos) d.dispose();
    this.decos = [];
    const buf = term.buffer.active;
    if (!enabled || buf.type !== "normal") return;
    const line = buf.getLine(buf.baseY + buf.cursorY);
    if (!line) return;
    const prompt = plainPrompt(line, matches(line.translateToString(true))).filter(([, , k]) => k === "user" || k === "root" || k === "host" || k === "path");
    if (prompt.length) decorate(term, line, 0, prompt, LIGHT_THEMES.has(theme) ? LIGHT : DARK, prompt.length, this.decos);
  }
}

/** Column of a string index (wide characters take two cells). */
function cellIndex(line: { length: number; getCell(x: number): { getChars(): string; getWidth(): number } | undefined }, index: number): number {
  let chars = 0;
  for (let x = 0; x < line.length; x++) {
    if (chars >= index) return x;
    const c = line.getCell(x);
    if (!c || c.getWidth() === 0) continue;
    chars += c.getChars().length || 1;
  }
  return index;
}
