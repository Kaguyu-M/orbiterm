// CodeMirror look shared by the editor and the comparison tabs: colours from the
// interface theme, syntax colours for light and dark themes, language from the file name.
import { EditorView } from "@codemirror/view";
import type { Extension } from "@codemirror/state";
import { HighlightStyle, LanguageDescription, syntaxHighlighting, defaultHighlightStyle } from "@codemirror/language";
import { languages } from "@codemirror/language-data";
import { tags as t } from "@lezer/highlight";

const darkHighlight = HighlightStyle.define([
  { tag: [t.keyword, t.operatorKeyword, t.modifier], color: "#e0a060" },
  { tag: [t.string, t.special(t.string), t.regexp], color: "#a8c97f" },
  { tag: [t.number, t.bool, t.null, t.atom], color: "#d79a6b" },
  { tag: [t.comment, t.meta], color: "#8a8478", fontStyle: "italic" },
  { tag: [t.function(t.variableName), t.function(t.propertyName)], color: "#e6c07b" },
  { tag: [t.typeName, t.className, t.namespace], color: "#d8b46a" },
  { tag: [t.propertyName, t.attributeName], color: "#c9b28f" },
  { tag: [t.tagName, t.heading], color: "#e08f6a", fontWeight: "bold" },
  { tag: [t.definition(t.variableName)], color: "#ece8e1" },
  { tag: t.invalid, color: "#ff6b5b" },
]);

export function appearance(dark: boolean): Extension {
  return [
    EditorView.theme(
      {
        "&": { height: "100%", backgroundColor: "var(--input-bg)", color: "var(--text)" },
        ".cm-scroller": { fontFamily: "Consolas, 'Cascadia Mono', 'DejaVu Sans Mono', monospace", fontSize: "13.5px", lineHeight: "1.45" },
        ".cm-gutters": { backgroundColor: "var(--chrome)", color: "var(--muted)", borderRight: "1px solid var(--chrome-border)" },
        ".cm-activeLine": { backgroundColor: "color-mix(in srgb, var(--accent) 7%, transparent)" },
        ".cm-activeLineGutter": { backgroundColor: "color-mix(in srgb, var(--accent) 14%, transparent)", color: "var(--text)" },
        ".cm-cursor": { borderLeftColor: "var(--text)" },
        "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection": {
          backgroundColor: "color-mix(in srgb, var(--accent) 30%, transparent) !important",
        },
        ".cm-panels": { backgroundColor: "var(--chrome)", color: "var(--text)", borderColor: "var(--chrome-border)" },
        ".cm-panel input, .cm-panel button": { fontSize: "12px" },
        ".cm-searchMatch": { backgroundColor: "color-mix(in srgb, var(--warn) 35%, transparent)" },
        ".cm-searchMatch-selected": { backgroundColor: "color-mix(in srgb, var(--accent) 45%, transparent)" },
      },
      { dark },
    ),
    syntaxHighlighting(dark ? darkHighlight : defaultHighlightStyle, { fallback: true }),
  ];
}


/** Language of a file name (loaded on demand), or plain text. */
export async function languageFor(name: string): Promise<{ name: string; support: Extension }> {
  const d = LanguageDescription.matchFilename(languages, name);
  if (!d) return { name: "Texte", support: [] };
  const support = await d.load().catch(() => null);
  return { name: d.name, support: support ?? [] };
}
