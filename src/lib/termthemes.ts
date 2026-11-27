// Colour themes of the terminals: the built-in ones and the user's own (made in the theme
// editor of the settings, saved with the preferences).

export type ThemeColors = Record<string, string>;

export interface CustomTheme {
  /** "custom:<random>" */
  id: string;
  name: string;
  colors: ThemeColors;
}

/** Keys of an xterm.js theme the editor shows, with their French labels. */
export const BASE_KEYS: Array<[string, string]> = [
  ["background", "Fond"],
  ["foreground", "Texte"],
  ["cursor", "Curseur"],
  ["cursorAccent", "Texte sous le curseur"],
  ["selectionBackground", "Sélection"],
];
export const ANSI_KEYS: Array<[string, string]> = [
  ["black", "Noir"],
  ["red", "Rouge"],
  ["green", "Vert"],
  ["yellow", "Jaune"],
  ["blue", "Bleu"],
  ["magenta", "Magenta"],
  ["cyan", "Cyan"],
  ["white", "Blanc"],
  ["brightBlack", "Noir vif"],
  ["brightRed", "Rouge vif"],
  ["brightGreen", "Vert vif"],
  ["brightYellow", "Jaune vif"],
  ["brightBlue", "Bleu vif"],
  ["brightMagenta", "Magenta vif"],
  ["brightCyan", "Cyan vif"],
  ["brightWhite", "Blanc vif"],
];

/** xterm.js defaults, for the colours a built-in theme leaves out. */
const XTERM_DEFAULTS: ThemeColors = {
  background: "#000000",
  foreground: "#ffffff",
  cursor: "#ffffff",
  cursorAccent: "#000000",
  selectionBackground: "#4a5a78",
  black: "#2e3436",
  red: "#cc0000",
  green: "#4e9a06",
  yellow: "#c4a000",
  blue: "#3465a4",
  magenta: "#75507b",
  cyan: "#06989a",
  white: "#d3d7cf",
  brightBlack: "#555753",
  brightRed: "#ef2929",
  brightGreen: "#8ae234",
  brightYellow: "#fce94f",
  brightBlue: "#729fcf",
  brightMagenta: "#ad7fa8",
  brightCyan: "#34e2e2",
  brightWhite: "#eeeeec",
};

export const BUILTIN_THEMES: Record<string, ThemeColors> = {
  dark: { background: "#000000", foreground: "#e6e6e6", cursor: "#e6e6e6", selectionBackground: "#4a5a78" },
  light: { background: "#ffffff", foreground: "#1d2027", cursor: "#1d2027", selectionBackground: "#b5d0f5",
    black: "#1d2027", brightBlack: "#5b6470", white: "#c9d1db", brightWhite: "#ffffff", yellow: "#a87b00", brightYellow: "#b88a00" },
  solarized: { background: "#002b36", foreground: "#93a1a1", cursor: "#93a1a1", selectionBackground: "#2a4a55" },
  // palette Orbiterm : brun profond / crème / ambre / olive
  orbiterm: { background: "#28251f", foreground: "#f3ede0", cursor: "#d9a35f", cursorAccent: "#28251f", selectionBackground: "#58402b",
    black: "#28251f", red: "#c0583f", green: "#8f9a5c", yellow: "#d9a35f", blue: "#7f95a8", magenta: "#b0806a", cyan: "#8fae9c", white: "#d8cfbd",
    brightBlack: "#62553b", brightRed: "#e07a5f", brightGreen: "#b5c07a", brightYellow: "#f0c27e", brightBlue: "#a3b8c9", brightMagenta: "#d4a38b", brightCyan: "#b3d1bf", brightWhite: "#f3ede0" },
  creme: { background: "#f3ede0", foreground: "#28251f", cursor: "#9c6f3e", cursorAccent: "#f3ede0", selectionBackground: "#e0cfae",
    black: "#28251f", red: "#a8432f", green: "#5a6b2e", yellow: "#9c6f3e", blue: "#3f5d78", magenta: "#7e4a5a", cyan: "#3f7466", white: "#cfc5b0",
    brightBlack: "#62553b", brightRed: "#c0583f", brightGreen: "#696b4d", brightYellow: "#b9853f", brightBlue: "#557a9b", brightMagenta: "#9b6276", brightCyan: "#4f8f7e", brightWhite: "#ffffff" },
  // néon : nuit bleu-violet, cyan / magenta / jaune
  cyber: { background: "#05060f", foreground: "#c8f6ff", cursor: "#ff2a8a", cursorAccent: "#05060f", selectionBackground: "#3a1a6e",
    black: "#0d0e1f", red: "#ff3b6b", green: "#3dffa2", yellow: "#fcee0a", blue: "#3d8bff", magenta: "#ff2a8a", cyan: "#00e5ff", white: "#c8d3ff",
    brightBlack: "#5b6190", brightRed: "#ff6e93", brightGreen: "#7dffc2", brightYellow: "#fff566", brightBlue: "#7fb2ff", brightMagenta: "#ff6fb5", brightCyan: "#7df3ff", brightWhite: "#ffffff" },
  // gris classique
  graphite: { background: "#2b2b2b", foreground: "#dcdcdc", cursor: "#dcdcdc", cursorAccent: "#2b2b2b", selectionBackground: "#214283",
    black: "#1e1e1e", red: "#e0605a", green: "#7bb662", yellow: "#d9a441", blue: "#5c9ad6", magenta: "#b07cc6", cyan: "#56b6c2", white: "#c8c8c8",
    brightBlack: "#777b7e", brightRed: "#f0817b", brightGreen: "#9bd17f", brightYellow: "#ebc06b", brightBlue: "#8ab4e8", brightMagenta: "#cba0dd", brightCyan: "#7fd2dc", brightWhite: "#ffffff" },
  "graphite-light": { background: "#ffffff", foreground: "#1e1e1e", cursor: "#1e1e1e", cursorAccent: "#ffffff", selectionBackground: "#cce4f7",
    black: "#1e1e1e", red: "#c42b1c", green: "#2e7d32", yellow: "#8f6a00", blue: "#0067c0", magenta: "#8e3a9d", cyan: "#00798c", white: "#c8c8c8",
    brightBlack: "#6b6b6b", brightRed: "#d9534a", brightGreen: "#3f9443", brightYellow: "#a67c00", brightBlue: "#2f86d6", brightMagenta: "#a857b6", brightCyan: "#1a93a6", brightWhite: "#ffffff" },
};

export const BUILTIN_LABELS: Array<[string, string]> = [
  ["dark", "Noir"],
  ["light", "Clair"],
  ["solarized", "Solarized"],
  ["orbiterm", "Orbiterm brun"],
  ["creme", "Orbiterm crème"],
  ["cyber", "Cyberpunk néon"],
  ["graphite", "Gris sombre"],
  ["graphite-light", "Gris clair"],
];

/** Colours of theme `id` (a custom theme unknown here falls back to "dark"). */
export function themeColors(id: string, custom: CustomTheme[] = []): ThemeColors {
  const c = custom.find((t) => t.id === id);
  if (c) return { ...c.colors };
  return { ...(BUILTIN_THEMES[id] ?? BUILTIN_THEMES.dark) };
}

/** Every colour of a theme, the ones it leaves out taken from xterm's defaults. */
export function fullColors(id: string, custom: CustomTheme[] = []): ThemeColors {
  return { ...XTERM_DEFAULTS, ...themeColors(id, custom) };
}

/** Options of a theme <select>: built-in themes, then the user's. */
export function themeChoices(custom: CustomTheme[] = []): Array<[string, string]> {
  return [...BUILTIN_LABELS, ...custom.map((t): [string, string] => [t.id, `${t.name} (personnalisé)`])];
}

/** Relative luminance of a #rrggbb colour (0 = black, 1 = white). */
export function luminance(hex: string): number {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) return 0;
  const n = parseInt(m[1], 16);
  const ch = [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((v) => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * ch[0] + 0.7152 * ch[1] + 0.0722 * ch[2];
}

/** Light background? (keyword colours of the terminal are darker then) */
export function isLight(id: string, custom: CustomTheme[] = []): boolean {
  return luminance(themeColors(id, custom).background ?? "#000000") > 0.5;
}

/** #rrggbb + opacity (0..1) → rgba(): background of a terminal drawn over an image. */
export function withAlpha(hex: string, alpha: number): string {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) return hex;
  const n = parseInt(m[1], 16);
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${Math.max(0, Math.min(1, alpha))})`;
}

/**
 * Colour scheme pasted by the user: Windows Terminal ("schemes" entry: background,
 * foreground, cursorColor, selectionBackground, black…brightWhite, "purple" for magenta) or
 * an xterm.js theme object. Returns the recognised colours, or null.
 */
export function parseScheme(text: string): { name: string; colors: ThemeColors } | null {
  let obj: unknown;
  try {
    obj = JSON.parse(text);
  } catch {
    return null;
  }
  if (obj && typeof obj === "object" && Array.isArray((obj as { schemes?: unknown }).schemes)) {
    obj = (obj as { schemes: unknown[] }).schemes[0];
  }
  if (!obj || typeof obj !== "object") return null;
  const src = obj as Record<string, unknown>;
  const alias: Record<string, string> = { cursorColor: "cursor", purple: "magenta", brightPurple: "brightMagenta" };
  const known = new Set([...BASE_KEYS, ...ANSI_KEYS].map(([k]) => k));
  const colors: ThemeColors = {};
  for (const [k, v] of Object.entries(src)) {
    const key = alias[k] ?? k;
    if (known.has(key) && typeof v === "string" && /^#[0-9a-f]{6}$/i.test(v.trim())) colors[key] = v.trim().toLowerCase();
  }
  if (!colors.background && !colors.foreground) return null;
  return { name: typeof src.name === "string" && src.name.trim() ? src.name.trim() : "Thème importé", colors };
}
