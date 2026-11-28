// Monospace fonts offered for the terminals. Four Nerd Fonts ship with Orbiterm (static/fonts,
// "Mono" variants: icons one cell wide); the list marks the other ones installed on this computer
// (Nerd Fonts add the icons of fancy prompts: Starship, Powerlevel10k, Oh My Posh…).

export interface FontChoice {
  /** CSS font-family value */
  family: string;
  /** name shown in the list */
  label: string;
}

const MONO_FALLBACK = "'DejaVu Sans Mono', monospace";

const NAMES = [
  "Cascadia Mono",
  "Cascadia Code",
  "Consolas",
  "JetBrains Mono",
  "Fira Code",
  "Fira Mono",
  "Hack",
  "Source Code Pro",
  "Ubuntu Mono",
  "DejaVu Sans Mono",
  "Liberation Mono",
  "Noto Sans Mono",
  "IBM Plex Mono",
  "Roboto Mono",
  "Inconsolata",
  "Iosevka",
  "Menlo",
  "Monaco",
  "Lucida Console",
  "Courier New",
  // Nerd Fonts (icons)
  "CaskaydiaCove Nerd Font",
  "CaskaydiaMono Nerd Font",
  "JetBrainsMono Nerd Font",
  "FiraCode Nerd Font",
  "Hack Nerd Font",
  "MesloLGS NF",
  "MesloLGS Nerd Font",
  "SauceCodePro Nerd Font",
  "UbuntuMono Nerd Font",
  "DejaVuSansM Nerd Font",
];

/** Fonts bundled with the app: name → file prefix in static/fonts (Regular + Bold). */
const BUNDLED: Record<string, string> = {
  "JetBrainsMono Nerd Font": "JetBrainsMonoNerdFontMono",
  "CaskaydiaCove Nerd Font": "CaskaydiaCoveNerdFontMono",
  "FiraCode Nerd Font": "FiraCodeNerdFontMono",
  "Hack Nerd Font": "HackNerdFontMono",
};

let registered = false;
/** Declares the bundled fonts to the page (read from the app only when first used). */
function register() {
  if (registered || typeof document === "undefined" || typeof FontFace === "undefined") return;
  registered = true;
  for (const [name, file] of Object.entries(BUNDLED)) {
    for (const [weight, style] of [["400", "Regular"], ["700", "Bold"]]) {
      document.fonts.add(new FontFace(name, `url(/fonts/${file}-${style}.woff2) format("woff2")`, { weight, display: "block" }));
    }
  }
}
register();

/** Whether `name` ships with Orbiterm. */
export function bundled(name: string): boolean {
  return name in BUNDLED;
}

/**
 * Loads the font of a CSS family (bundled fonts are read on first use). xterm measures the
 * cells with the font it has when opened: call this first, or re-measure afterwards.
 */
export async function ensureLoaded(family: string): Promise<boolean> {
  const name = firstName(family);
  if (!bundled(name) || typeof document === "undefined") return false;
  try {
    await Promise.all([document.fonts.load(`16px "${name}"`), document.fonts.load(`bold 16px "${name}"`)]);
    return true;
  } catch {
    return false;
  }
}

/** CSS family of a font name, with fallbacks (the first one Orbiterm always used stays as it was). */
export function familyOf(name: string): string {
  if (name === "Cascadia Mono") return "Cascadia Mono, Consolas, 'DejaVu Sans Mono', monospace";
  return `'${name.replace(/'/g, "")}', ${MONO_FALLBACK}`;
}

/** First font name of a CSS family value. */
export function firstName(family: string): string {
  return family.split(",")[0].trim().replace(/^['"]|['"]$/g, "");
}

let canvas: HTMLCanvasElement | undefined;
/** Whether `name` is installed: text drawn with it differs from the fallback fonts. */
export function installed(name: string): boolean {
  if (bundled(name)) return true;
  canvas ??= document.createElement("canvas");
  const ctx = canvas.getContext("2d");
  if (!ctx) return true;
  const sample = "mmmmmmmmmmlli10OQ@#";
  return ["monospace", "serif", "sans-serif"].some((base) => {
    ctx.font = `40px ${base}`;
    const ref = ctx.measureText(sample).width;
    ctx.font = `40px '${name}', ${base}`;
    return ctx.measureText(sample).width !== ref;
  });
}

/** The list for a <select>: fonts shipped with Orbiterm, then the installed ones, then the others. */
export function fontChoices(): Array<FontChoice & { installed: boolean }> {
  const ship = Object.keys(BUNDLED).map((n) => ({ family: familyOf(n), label: `${n} (intégrée)`, installed: true }));
  const all = NAMES.filter((n) => !bundled(n)).map((n) => ({ family: familyOf(n), label: n, installed: installed(n) }));
  return [...ship, ...all.filter((f) => f.installed), ...all.filter((f) => !f.installed)];
}
