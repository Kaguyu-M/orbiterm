/** Multiple selection in a file list, as in a file manager. */
export interface Selection {
  /** selected ids */
  ids: string[];
  /** row a Shift+click extends from */
  anchor: string | null;
}

/**
 * Click on a row: a plain click selects that row only, Ctrl (Cmd) toggles it, Shift selects
 * the range from the anchor (added to the selection with Ctrl+Shift). `order` = ids as displayed.
 */
export function clickSelect(order: string[], cur: Selection, id: string, ev: { ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }): Selection {
  const ctrl = ev.ctrlKey || ev.metaKey;
  if (ev.shiftKey && cur.anchor && order.includes(cur.anchor)) {
    const a = order.indexOf(cur.anchor);
    const b = order.indexOf(id);
    const range = order.slice(Math.min(a, b), Math.max(a, b) + 1);
    return { ids: ctrl ? [...new Set([...cur.ids, ...range])] : range, anchor: cur.anchor };
  }
  if (ctrl) return { ids: cur.ids.includes(id) ? cur.ids.filter((x) => x !== id) : [...cur.ids, id], anchor: id };
  return { ids: [id], anchor: id };
}

/** "3 éléments" / the single name, for labels and confirmations. */
export function countLabel(names: string[]): string {
  return names.length === 1 ? `« ${names[0]} »` : `${names.length} éléments`;
}
