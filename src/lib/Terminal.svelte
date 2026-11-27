<script lang="ts" module>
  /** Controls of every mounted terminal, by tab key (used by menus / toolbar). */
  export interface TermControls {
    copy: () => void;
    paste: () => void;
    clear: () => void;
    selectAll: () => void;
    zoom: (delta: number) => void;
    focus: () => void;
    write: (text: string) => void;
    search: () => void;
  }
  export const terminals = new Map<string, TermControls>();
</script>

<script lang="ts">
  // One xterm.js terminal bound to one backend connection (any protocol).
  import { clipRead, clipWrite } from "./clipboard";
  import { ZmodemBridge } from "./zmodem";
  import { onMount } from "svelte";
  import { Terminal, type FontWeight } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { WebLinksAddon } from "@xterm/addon-web-links";
  import { SearchAddon } from "@xterm/addon-search";
  import { CursorPrompt, highlightLines, LineTracker, linesText } from "./highlight";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import "@xterm/xterm/css/xterm.css";
  import { api, type TerminalSettings } from "./api";
  import { attach } from "./bus";
  import type { Prefs } from "./app.svelte";
  import { actionOf } from "./shortcuts";
  import { imageUrl } from "./termbg";
  import { isLight, themeColors, withAlpha } from "./termthemes";

  let {
    tabKey,
    connId,
    settings,
    active,
    status,
    prefs,
    oninput,
  }: {
    tabKey: string;
    connId: string | null;
    settings: TerminalSettings;
    active: boolean;
    status: string;
    /** global terminal behaviour (cursor, copy/paste, bell) */
    prefs: Prefs;
    /** Extra listener for typed data (MultiExec, macros). */
    oninput?: (data: string) => void;
  } = $props();
  let flash = $state(false);

  let host: HTMLDivElement;
  let term: Terminal | undefined;
  let fit: FitAddon | undefined;
  let attachedTo: string | null = null;
  let detach: (() => void) | undefined;
  let lastSize = "";
  let hlLines: LineTracker | undefined;
  let logLines: LineTracker | undefined;
  /** session log requested when this connection was opened */
  let logging = false;
  function flushLog(withCursorLine: boolean) {
    if (!logging || !term || !connId) return;
    const r = logLines?.take(withCursorLine);
    if (r) api.termLog(connId, linesText(term, r)).catch(() => {});
  }
  // search bar (Ctrl+Shift+F)
  let search: SearchAddon | undefined;
  let searchOpen = $state(false);
  let query = $state("");
  let matchCase = $state(false);
  let found = $state(""); // "3 / 12", "aucun résultat"
  let searchInput: HTMLInputElement | undefined = $state();
  const SEARCH_DECO = {
    matchBackground: "#7a5a2a",
    activeMatchBackground: "#d9a35f",
    matchOverviewRuler: "#d9a35f",
    activeMatchColorOverviewRuler: "#f0c27e",
  };
  function openSearch() {
    searchOpen = true;
    const sel = term?.getSelection();
    if (sel && !sel.includes("\n")) query = sel;
    requestAnimationFrame(() => {
      searchInput?.focus();
      searchInput?.select();
    });
  }
  function find(back = false) {
    if (!search) return;
    if (!query) {
      search.clearDecorations();
      found = "";
      return;
    }
    const opts = { caseSensitive: matchCase, decorations: SEARCH_DECO };
    const ok = back ? search.findPrevious(query, opts) : search.findNext(query, opts);
    if (!ok) found = "aucun résultat";
  }
  function closeSearch() {
    searchOpen = false;
    search?.clearDecorations();
    term?.clearSelection();
    found = "";
    term?.focus();
  }
  // colours (built-in or custom theme), and the background image of the settings
  const colors = $derived(themeColors(settings.theme, prefs.customThemes));
  const light = $derived(isLight(settings.theme, prefs.customThemes));
  const tone = $derived(light ? "light" : "dark");
  let bgUrl = $state("");
  $effect(() => {
    const path = prefs.termBgImage;
    if (!path) {
      bgUrl = "";
      return;
    }
    imageUrl(path).then((u) => (bgUrl = u)).catch(() => (bgUrl = ""));
  });
  /** colour of the cells: transparent over an image (the veil is drawn by the frame, margins included) */
  const cellBg = $derived(bgUrl ? "rgba(0, 0, 0, 0)" : (colors.background ?? "#000000"));
  const veil = $derived(withAlpha(colors.background ?? "#000000", prefs.termBgOpacity / 100));
  const FITS = { cover: "center / cover no-repeat", contain: "center / contain no-repeat", center: "center no-repeat", repeat: "repeat" };
  const hostBg = $derived(
    bgUrl ? `linear-gradient(${veil}, ${veil}), url("${bgUrl}") ${FITS[prefs.termBgFit] ?? FITS.cover}, ${colors.background ?? "#000000"}` : cellBg,
  );
  const xtermTheme = $derived({ ...colors, background: cellBg });

  function sendSize() {
    if (!term || !connId) return;
    const size = `${term.cols}x${term.rows}`;
    if (size === lastSize) return;
    lastSize = size;
    api.termResize(connId, term.cols, term.rows).catch(() => {});
  }

  function refit() {
    if (!fit || !host || host.offsetWidth === 0) return;
    try {
      fit.fit();
      sendSize();
    } catch {
      /* not visible yet */
    }
  }

  let zmodem: ZmodemBridge | null = null;

  function send(d: string) {
    if (connId) api.termWrite(connId, d).catch(() => {});
  }

  onMount(() => {
    term = new Terminal({
      fontFamily: settings.font_family,
      fontSize: settings.font_size,
      scrollback: settings.scrollback,
      cursorBlink: prefs.cursorBlink,
      cursorStyle: prefs.cursorStyle,
      lineHeight: prefs.termLineHeight || 1,
      letterSpacing: prefs.termLetterSpacing || 0,
      fontWeight: (prefs.termFontWeight || "normal") as FontWeight,
      fontWeightBold: (prefs.termFontWeightBold || "bold") as FontWeight,
      allowTransparency: true,
      allowProposedApi: true,
      theme: xtermTheme,
    });
    fit = new FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new WebLinksAddon((_e, uri) => openUrl(uri)));
    search = new SearchAddon();
    term.loadAddon(search);
    search.onDidChangeResults(({ resultIndex, resultCount }) => {
      found = resultCount === 0 ? "aucun résultat" : resultIndex >= 0 ? `${resultIndex + 1} / ${resultCount}` : `${resultCount}`;
    });
    term.open(host);
    // completed lines: keywords in colour (error / warning / success) and session log
    hlLines = new LineTracker(term, false);
    const cursorPrompt = new CursorPrompt();
    logLines = new LineTracker(term, true);
    let scanQueued = false;
    term.onWriteParsed(() => {
      if (scanQueued) return;
      scanQueued = true;
      requestAnimationFrame(() => {
        scanQueued = false;
        if (!term) return;
        const r = hlLines!.take();
        if (r && prefs.termHighlight) highlightLines(term, r, tone);
        cursorPrompt.update(term, tone, prefs.termHighlight);
        flushLog(false);
      });
    });

    term.onData((d) => {
      // during a ZMODEM transfer the keyboard only interrupts it (Ctrl+C)
      if (zmodem?.active) {
        if (d.charCodeAt(0) === 3) zmodem.abort();
        return;
      }
      send(d);
      oninput?.(d);
    });

    const copy = () => {
      const sel = term!.getSelection();
      if (sel) clipWrite(sel);
    };
    const paste = () => clipRead().then((t) => t && term!.paste(t));

    // Copy / paste / search on the user's shortcuts (by default Ctrl+Shift+C, Ctrl+Shift+V or
    // Shift+Insert, Ctrl+Shift+F; Ctrl+C stays "interrupt").
    term.attachCustomKeyEventHandler((ev) => {
      const action = actionOf(prefs.keys, ev);
      const pasteKey = action === "paste";
      const copyKey = action === "copy";
      if (action === "search") {
        if (ev.type === "keydown") openSearch();
        ev.preventDefault();
        return false;
      }
      // the other shortcuts of the application (tabs, zoom…) are not typed into the shell
      if (action && !pasteKey && !copyKey) return false;
      if (!pasteKey && !copyKey) return true;
      // The WebView would also paste natively ("paste as plain text" on Ctrl+Shift+V,
      // Shift+Insert): xterm then received the text a second time. Only our handler acts.
      ev.preventDefault();
      if (ev.type === "keydown") {
        if (pasteKey) paste();
        else copy();
      }
      return false;
    });
    // right click = paste, like most Unix terminals (selection is copied automatically)
    host.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      if (prefs.rightClickPaste) paste();
    });
    term.onSelectionChange(() => {
      if (!prefs.copyOnSelect) return;
      const sel = term!.getSelection();
      if (sel) clipWrite(sel).catch(() => {});
    });
    term.onBell(() => {
      if (!prefs.visualBell) return;
      flash = true;
      setTimeout(() => (flash = false), 150);
    });

    terminals.set(tabKey, {
      copy,
      paste,
      clear: () => term!.clear(),
      selectAll: () => term!.selectAll(),
      zoom: (delta) => {
        const size = delta === 0 ? settings.font_size : Math.min(40, Math.max(6, (term!.options.fontSize ?? 14) + delta));
        term!.options.fontSize = size;
        refit();
      },
      focus: () => term!.focus(),
      write: (text) => send(text),
      search: openSearch,
    });

    const ro = new ResizeObserver(() => refit());
    ro.observe(host);
    refit();

    return () => {
      terminals.delete(tabKey);
      ro.disconnect();
      detach?.();
      term?.dispose();
    };
  });

  // Attach to the connection as soon as we have its id.
  $effect(() => {
    if (!term || !connId || attachedTo === connId) return;
    detach?.();
    attachedTo = connId;
    lastSize = "";
    logging = prefs.termLog;
    const id = connId;
    zmodem = new ZmodemBridge({ write: (b) => term!.write(b), connId: () => id });
    const zm = zmodem;
    detach = attach(connId, {
      data: (bytes) => zm.consume(bytes),
      closed: (reason) => {
        // the last lines (prompt included) before the backend closes the file
        term!.write("", () => flushLog(true));
        term!.write(`\r\n\x1b[33m[${reason}]\x1b[0m\r\n`);
      },
    });
    refit();
    sendSize();
  });

  // Global settings changed in the configuration dialog.
  $effect(() => {
    const style = prefs.cursorStyle;
    const blink = prefs.cursorBlink;
    if (term) {
      term.options.cursorStyle = style;
      term.options.cursorBlink = blink;
    }
  });
  $effect(() => {
    const o = {
      lineHeight: prefs.termLineHeight || 1,
      letterSpacing: prefs.termLetterSpacing || 0,
      fontWeight: (prefs.termFontWeight || "normal") as FontWeight,
      fontWeightBold: (prefs.termFontWeightBold || "bold") as FontWeight,
      theme: xtermTheme,
    };
    if (!term) return;
    Object.assign(term.options, o);
    refit();
  });

  // Becoming visible: re-fit and focus.
  $effect(() => {
    if (active && term) {
      requestAnimationFrame(() => {
        refit();
        term!.focus();
      });
    }
  });

  // Status messages (connecting, errors) shown inside the terminal.
  let lastStatus = "";
  $effect(() => {
    if (term && status && status !== lastStatus) {
      lastStatus = status;
      term.write(`\x1b[90m${status}\x1b[0m\r\n`);
    }
  });
</script>

<div class="term" class:flash bind:this={host} style:background={hostBg} style:--term-bg={cellBg}></div>
{#if searchOpen}
  <div class="find" role="search">
    <input
      bind:this={searchInput}
      bind:value={query}
      placeholder="Rechercher dans le terminal"
      spellcheck="false"
      oninput={() => find()}
      onkeydown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          find(e.shiftKey);
        } else if (e.key === "Escape") {
          e.preventDefault();
          closeSearch();
        }
      }}
    />
    <span class="count">{found}</span>
    <button class:on={matchCase} title="Respecter la casse" onclick={() => { matchCase = !matchCase; find(); }}>Aa</button>
    <button title="Précédent (Maj+Entrée)" onclick={() => find(true)}>▲</button>
    <button title="Suivant (Entrée)" onclick={() => find()}>▼</button>
    <button title="Fermer (Échap)" onclick={closeSearch}>✕</button>
  </div>
{/if}

<style>
  .term {
    position: absolute;
    inset: 0;
    padding: 3px 0 0 5px;
  }
  .find {
    position: absolute;
    top: 6px;
    right: 18px;
    z-index: 5;
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px 6px;
    background: var(--panel, #2f2a22);
    border: 1px solid var(--border, #5a4f3c);
    border-radius: 6px;
    box-shadow: 0 4px 14px rgba(0, 0, 0, 0.35);
    font-size: 12px;
  }
  .find input {
    width: 210px;
    padding: 3px 6px;
  }
  .find .count {
    min-width: 74px;
    color: var(--muted, #a99c84);
    text-align: center;
  }
  .find button {
    padding: 2px 7px;
    min-width: 26px;
  }
  .find button.on {
    background: var(--accent, #d9a35f);
    color: #28251f;
  }
  .term.flash {
    outline: 3px solid var(--warn);
    outline-offset: -3px;
  }
  .term :global(.xterm) {
    height: 100%;
  }
  .term :global(.xterm-viewport::-webkit-scrollbar-track),
  .term :global(.xterm-viewport::-webkit-scrollbar-corner),
  .term :global(.xterm-scrollable-element::-webkit-scrollbar-corner) {
    background: transparent;
  }
  .term :global(.xterm-viewport::-webkit-scrollbar-thumb) {
    background: #6f675a;
    border-color: transparent;
  }
  .term :global(.xterm-viewport) {
    background-color: var(--term-bg) !important;
  }
</style>
