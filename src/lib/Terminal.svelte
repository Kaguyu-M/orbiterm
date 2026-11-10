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
  }
  export const terminals = new Map<string, TermControls>();
</script>

<script lang="ts">
  // One xterm.js terminal bound to one backend connection (any protocol).
  import { clipRead, clipWrite } from "./clipboard";
  import { onMount } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { WebLinksAddon } from "@xterm/addon-web-links";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import "@xterm/xterm/css/xterm.css";
  import { api, type TerminalSettings } from "./api";
  import { attach } from "./bus";
  import type { Prefs } from "./app.svelte";

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

  const THEMES: Record<string, Record<string, string>> = {
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
  };

  let host: HTMLDivElement;
  let term: Terminal | undefined;
  let fit: FitAddon | undefined;
  let attachedTo: string | null = null;
  let detach: (() => void) | undefined;
  let lastSize = "";
  const bg = $derived((THEMES[settings.theme] ?? THEMES.dark).background);

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
      allowProposedApi: true,
      theme: THEMES[settings.theme] ?? THEMES.dark,
    });
    fit = new FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new WebLinksAddon((_e, uri) => openUrl(uri)));
    term.open(host);

    term.onData((d) => {
      send(d);
      oninput?.(d);
    });

    const copy = () => {
      const sel = term!.getSelection();
      if (sel) clipWrite(sel);
    };
    const paste = () => clipRead().then((t) => t && term!.paste(t));

    // Copy on Ctrl+Shift+C, paste on Ctrl+Shift+V / Shift+Insert (Ctrl+C stays "interrupt").
    term.attachCustomKeyEventHandler((ev) => {
      const pasteKey = (ev.shiftKey && !ev.ctrlKey && ev.code === "Insert") || (ev.ctrlKey && ev.shiftKey && ev.code === "KeyV");
      const copyKey = ev.ctrlKey && ev.shiftKey && ev.code === "KeyC";
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
    detach = attach(connId, {
      data: (bytes) => term!.write(bytes),
      closed: (reason) => term!.write(`\r\n\x1b[33m[${reason}]\x1b[0m\r\n`),
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

<div class="term" class:flash bind:this={host} style:background={bg} style:--term-bg={bg}></div>

<style>
  .term {
    position: absolute;
    inset: 0;
    padding: 3px 0 0 5px;
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
