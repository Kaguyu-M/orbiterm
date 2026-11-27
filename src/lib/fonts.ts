// Monospace fonts offered for the terminals. None is bundled: the list marks the ones
// installed on this computer (Nerd Fonts add the icons of fancy prompts: Starship, Powerlevel10k…).

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

/** The list for a <select>, installed fonts first. */
export function fontChoices(): Array<FontChoice & { installed: boolean }> {
  const all = NAMES.map((n) => ({ family: familyOf(n), label: n, installed: installed(n) }));
  return [...all.filter((f) => f.installed), ...all.filter((f) => !f.installed)];
}
