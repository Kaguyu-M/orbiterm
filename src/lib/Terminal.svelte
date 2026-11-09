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
  import { onMount } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { WebLinksAddon } from "@xterm/addon-web-links";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import "@xterm/xterm/css/xterm.css";
  import { api, type TerminalSettings } from "./api";
  import { attach } from "./bus";

  let {
    tabKey,
    connId,
    settings,
    active,
    status,
    oninput,
  }: {
    tabKey: string;
    connId: string | null;
    settings: TerminalSettings;
    active: boolean;
    status: string;
    /** Extra listener for typed data (MultiExec). */
    oninput?: (data: string) => void;
  } = $props();

  const THEMES: Record<string, Record<string, string>> = {
    dark: { background: "#000000", foreground: "#e6e6e6", cursor: "#e6e6e6", selectionBackground: "#4a5a78" },
    light: { background: "#ffffff", foreground: "#1d2027", cursor: "#1d2027", selectionBackground: "#b5d0f5",
      black: "#1d2027", brightBlack: "#5b6470", white: "#c9d1db", brightWhite: "#ffffff", yellow: "#a87b00", brightYellow: "#b88a00" },
    solarized: { background: "#002b36", foreground: "#93a1a1", cursor: "#93a1a1", selectionBackground: "#2a4a55" },
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
      cursorBlink: true,
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
      if (sel) navigator.clipboard.writeText(sel);
    };
    const paste = () => navigator.clipboard.readText().then((t) => t && term!.paste(t));

    // Copy on Ctrl+Shift+C, paste on Ctrl+Shift+V / Shift+Insert (Ctrl+C stays "interrupt").
    term.attachCustomKeyEventHandler((ev) => {
      if (ev.type !== "keydown") return true;
      if (ev.shiftKey && ev.code === "Insert") {
        paste();
        return false;
      }
      if (!ev.ctrlKey || !ev.shiftKey) return true;
      if (ev.code === "KeyC") {
        copy();
        return false;
      }
      if (ev.code === "KeyV") {
        paste();
        return false;
      }
      return true;
    });
    // right click = paste, like most Unix terminals (selection is copied automatically)
    host.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      paste();
    });
    term.onSelectionChange(() => {
      const sel = term!.getSelection();
      if (sel) navigator.clipboard.writeText(sel).catch(() => {});
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

<div class="term" bind:this={host} style:background={bg} style:--term-bg={bg}></div>

<style>
  .term {
    position: absolute;
    inset: 0;
    padding: 3px 0 0 5px;
  }
  .term :global(.xterm) {
    height: 100%;
  }
  .term :global(.xterm-viewport) {
    background-color: var(--term-bg) !important;
  }
</style>
