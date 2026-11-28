// Monospace fonts offered for the terminals. The free ones ship with Orbiterm (static/fonts): their
// Nerd Fonts build, "Mono" variant (icons one cell wide, for Starship, Powerlevel10k, Oh My Posh…),
// declared under their usual name and under their Nerd Fonts name. Consolas, Lucida Console and
// Courier New belong to Microsoft and cannot be redistributed: used when the system has them.

export interface FontChoice {
  /** CSS font-family value */
  family: string;
  /** name shown in the list */
  label: string;
}

const MONO_FALLBACK = "'DejaVu Sans Mono', monospace";

/** Fonts bundled with the app: name, file prefix in static/fonts (Regular + Bold), other names. */
const BUNDLED: Array<[string, string, string[]]> = [
  ["Cascadia Mono", "CaskaydiaMonoNerdFontMono", ["CaskaydiaMono Nerd Font"]],
  ["Cascadia Code", "CaskaydiaCoveNerdFontMono", ["CaskaydiaCove Nerd Font"]],
  ["JetBrains Mono", "JetBrainsMonoNerdFontMono", ["JetBrainsMono Nerd Font"]],
  ["Fira Code", "FiraCodeNerdFontMono", ["FiraCode Nerd Font"]],
  ["Fira Mono", "FiraMonoNerdFontMono", ["FiraMono Nerd Font"]],
  ["Hack", "HackNerdFontMono", ["Hack Nerd Font"]],
  ["Source Code Pro", "SauceCodeProNerdFontMono", ["SauceCodePro Nerd Font"]],
  ["Ubuntu Mono", "UbuntuMonoNerdFontMono", ["UbuntuMono Nerd Font"]],
  ["DejaVu Sans Mono", "DejaVuSansMNerdFontMono", ["DejaVuSansM Nerd Font"]],
  ["Liberation Mono", "LiterationMonoNerdFontMono", ["LiterationMono Nerd Font"]],
  ["Noto Sans Mono", "NotoSansMNerdFontMono", ["NotoSansM Nerd Font"]],
  ["IBM Plex Mono", "BlexMonoNerdFontMono", ["BlexMono Nerd Font"]],
  ["Roboto Mono", "RobotoMonoNerdFontMono", ["RobotoMono Nerd Font"]],
  ["Inconsolata", "InconsolataNerdFontMono", ["Inconsolata Nerd Font"]],
  ["Iosevka", "IosevkaNerdFontMono", ["Iosevka Nerd Font"]],
  ["MesloLGS NF", "MesloLGSNerdFontMono", ["MesloLGS Nerd Font"]],
];

/** System fonts that cannot be shipped (Windows). */
const SYSTEM = ["Consolas", "Lucida Console", "Courier New"];

/** every name a bundled font answers to → its usual name */
const CANONICAL = new Map<string, string>();
for (const [name, , aliases] of BUNDLED) {
  CANONICAL.set(name.toLowerCase(), name);
  for (const a of aliases) CANONICAL.set(a.toLowerCase(), name);
}

let registered = false;
/** Declares the bundled fonts to the page (read from the app only when first used). */
function register() {
  if (registered || typeof document === "undefined" || typeof FontFace === "undefined") return;
  registered = true;
  for (const [name, file, aliases] of BUNDLED) {
    for (const family of [name, ...aliases]) {
      for (const [weight, style] of [["400", "Regular"], ["700", "Bold"]]) {
        document.fonts.add(new FontFace(family, `url(/fonts/${file}-${style}.woff2) format("woff2")`, { weight, display: "block" }));
      }
    }
  }
}
register();

/** Whether `name` ships with Orbiterm. */
export function bundled(name: string): boolean {
  return CANONICAL.has(name.toLowerCase());
}

/** The names of a CSS family value. */
function names(family: string): string[] {
  return family.split(",").map((n) => n.trim().replace(/^['"]|['"]$/g, "")).filter(Boolean);
}

/**
 * Loads the bundled fonts of a CSS family (read on first use). xterm measures the cells with
 * the font it has when opened: call this first, or re-measure afterwards. True when one loaded.
 */
export async function ensureLoaded(family: string): Promise<boolean> {
  if (typeof document === "undefined") return false;
  // the first font the system has decides: nothing to load behind an installed system font
  const list = names(family);
  const first = list.find((n) => bundled(n) || SYSTEM.includes(n));
  if (!first || !bundled(first)) return false;
  try {
    await Promise.all([document.fonts.load(`16px "${first}"`), document.fonts.load(`bold 16px "${first}"`)]);
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
  return names(family)[0] ?? "";
}

/** The family value of the list for a bundled font saved under another of its names. */
export function canonical(family: string): string {
  const name = CANONICAL.get(firstName(family).toLowerCase());
  return name && name !== firstName(family) ? familyOf(name) : family;
}

let canvas: HTMLCanvasElement | undefined;
/** Whether `name` is available: bundled, or drawn differently from the fallback fonts. */
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

/** The list for a <select>: the fonts shipped with Orbiterm, then the system ones. */
export function fontChoices(): Array<FontChoice & { installed: boolean }> {
  const ship = BUNDLED.map(([n]) => ({ family: familyOf(n), label: n, installed: true }));
  const sys = SYSTEM.map((n) => ({ family: familyOf(n), label: `${n} (système)`, installed: installed(n) }));
  return [...ship, ...sys];
}
