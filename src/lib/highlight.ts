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

const RULES: Array<{ re: RegExp; kind: "err" | "warn" | "ok" }> = [
  {
    re: word("errors?|erreurs?|fail(?:ed|ure|s)?|échecs?|échoué|fatal|denied|refusée?s?|critical|critique|panic|exception|abort(?:ed)?|segmentation fault"),
    kind: "err",
  },
  { re: word("warnings?|warn|attention|avertissements?|deprecated|obsolète"), kind: "warn" },
  { re: word("ok|success(?:ful(?:ly)?)?|succès|réussie?s?|passed|completed|terminée?"), kind: "ok" },
];

const DARK = { err: "#ff6b5b", warn: "#f0b45e", ok: "#9cc96b" };
const LIGHT = { err: "#b3261e", warn: "#9a6200", ok: "#3f6f1f" };
const LIGHT_THEMES = new Set(["light", "creme"]);

/** Max decorations created per pass (very fast output: the rest is skipped, not queued). */
const BUDGET = 400;

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
    for (const { re, kind } of RULES) {
      re.lastIndex = 0;
      for (const m of text.matchAll(re)) {
        if (budget-- <= 0) break;
        const marker = term.registerMarker(y - cursorLine);
        if (!marker) continue;
        const deco = term.registerDecoration({
          marker,
          x: cellIndex(line, m.index ?? 0),
          width: m[0].length,
          foregroundColor: colors[kind],
          layer: "top",
        });
        if (!deco) marker.dispose();
        else deco.onDispose(() => marker.dispose());
      }
    }
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
