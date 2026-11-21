// Keyboard shortcuts of the application, changeable in "Aide → Raccourcis clavier".
// A shortcut is written "Ctrl+Alt+Shift+Meta+<code>" with the physical key's `KeyboardEvent.code`
// (same key whatever the layout: AZERTY, QWERTY…). The user's choices are stored in the
// preferences (prefs.keys), only for the actions they changed.

export type ActionId =
  | "newTerminal"
  | "newSession"
  | "openEditor"
  | "closeTab"
  | "nextTab"
  | "prevTab"
  | "copy"
  | "paste"
  | "search"
  | "zoomIn"
  | "zoomOut"
  | "zoomReset"
  | "sidebar"
  | "fullscreen";

export interface ActionDef {
  id: ActionId;
  label: string;
  keys: string[];
}

export const ACTIONS: ActionDef[] = [
  { id: "newTerminal", label: "Nouveau terminal local", keys: ["Ctrl+Shift+KeyT"] },
  { id: "newSession", label: "Nouvelle session", keys: ["Ctrl+Shift+KeyN"] },
  { id: "openEditor", label: "Ouvrir un fichier dans l'éditeur", keys: ["Ctrl+Shift+KeyO"] },
  { id: "closeTab", label: "Fermer l'onglet", keys: ["Ctrl+Shift+KeyW"] },
  { id: "nextTab", label: "Onglet suivant", keys: ["Ctrl+Tab"] },
  { id: "prevTab", label: "Onglet précédent", keys: ["Ctrl+Shift+Tab"] },
  { id: "copy", label: "Copier (terminal)", keys: ["Ctrl+Shift+KeyC"] },
  { id: "paste", label: "Coller (terminal)", keys: ["Ctrl+Shift+KeyV", "Shift+Insert"] },
  { id: "search", label: "Rechercher (terminal)", keys: ["Ctrl+Shift+KeyF"] },
  { id: "zoomIn", label: "Agrandir le texte", keys: ["Ctrl+Shift+Equal", "Ctrl+Shift+NumpadAdd"] },
  { id: "zoomOut", label: "Réduire le texte", keys: ["Ctrl+Shift+Minus", "Ctrl+Shift+NumpadSubtract"] },
  { id: "zoomReset", label: "Taille normale du texte", keys: ["Ctrl+Shift+Digit0", "Ctrl+Shift+Numpad0"] },
  { id: "sidebar", label: "Afficher / masquer la barre latérale", keys: ["Ctrl+Shift+KeyB"] },
  { id: "fullscreen", label: "Plein écran", keys: ["F11"] },
];

export type KeyMap = Partial<Record<ActionId, string[]>>;

/** Shortcuts of an action: the user's choice, else the default. */
export function keysOf(map: KeyMap | undefined, id: ActionId): string[] {
  return map?.[id] ?? ACTIONS.find((a) => a.id === id)?.keys ?? [];
}

const MODIFIER_CODES = ["ControlLeft", "ControlRight", "AltLeft", "AltRight", "ShiftLeft", "ShiftRight", "MetaLeft", "MetaRight", "OSLeft", "OSRight"];

/** The shortcut of a key press, or "" for a modifier alone. */
export function comboOf(e: Pick<KeyboardEvent, "code" | "ctrlKey" | "altKey" | "shiftKey" | "metaKey">): string {
  if (!e.code || MODIFIER_CODES.includes(e.code)) return "";
  return [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Meta", e.code].filter(Boolean).join("+");
}

export function matches(map: KeyMap | undefined, id: ActionId, e: KeyboardEvent): boolean {
  const c = comboOf(e);
  return !!c && keysOf(map, id).includes(c);
}

/** The action a key press triggers, if any. */
export function actionOf(map: KeyMap | undefined, e: KeyboardEvent): ActionId | undefined {
  const c = comboOf(e);
  if (!c) return undefined;
  return ACTIONS.find((a) => keysOf(map, a.id).includes(c))?.id;
}

/** Why a shortcut cannot be used ("" = fine): without Ctrl, Alt or the system key, a key
 *  would be taken from typing, except function keys and a few that type nothing. */
export function refusal(combo: string): string {
  const parts = combo.split("+");
  const code = parts[parts.length - 1];
  const mods = parts.slice(0, -1);
  if (mods.some((m) => m === "Ctrl" || m === "Alt" || m === "Meta")) return "";
  if (/^F\d{1,2}$/.test(code) || ["Insert", "Pause", "ScrollLock", "PrintScreen"].includes(code)) return "";
  return "Ajoutez Ctrl, Alt ou une touche de fonction (F1…F12) : seule, cette touche ne pourrait plus être tapée.";
}

const KEY_NAMES: Record<string, string> = {
  Equal: "=",
  Minus: "-",
  NumpadAdd: "+ (pavé num.)",
  NumpadSubtract: "- (pavé num.)",
  NumpadMultiply: "* (pavé num.)",
  NumpadDivide: "/ (pavé num.)",
  NumpadEnter: "Entrée (pavé num.)",
  Insert: "Inser",
  Delete: "Suppr",
  Backspace: "Retour arrière",
  Enter: "Entrée",
  Escape: "Échap",
  Space: "Espace",
  ArrowUp: "Haut",
  ArrowDown: "Bas",
  ArrowLeft: "Gauche",
  ArrowRight: "Droite",
  PageUp: "Page préc.",
  PageDown: "Page suiv.",
  Home: "Début",
  End: "Fin",
  Pause: "Pause",
  Comma: ",",
  Period: ".",
  Semicolon: ";",
  Quote: "'",
  Slash: "/",
  Backslash: "\\",
  BracketLeft: "[",
  BracketRight: "]",
  Backquote: "`",
  IntlBackslash: "<",
};

/** Readable form: "Ctrl+Maj+T", "F11", "Ctrl+Maj++ (pavé num.)". */
export function label(combo: string, isWindows = true): string {
  return combo
    .split("+")
    .map((p) => {
      if (p === "Shift") return "Maj";
      if (p === "Meta") return isWindows ? "Win" : "Super";
      if (p.startsWith("Key")) return p.slice(3);
      if (p.startsWith("Digit")) return p.slice(5);
      if (p.startsWith("Numpad") && /\d$/.test(p)) return `${p.slice(6)} (pavé num.)`;
      return KEY_NAMES[p] ?? p;
    })
    .join("+");
}

/** First shortcut of an action, for the menus ("" when it has none). */
export function menuLabel(map: KeyMap | undefined, id: ActionId, isWindows = true): string {
  const k = keysOf(map, id)[0];
  return k ? label(k, isWindows) : "";
}
