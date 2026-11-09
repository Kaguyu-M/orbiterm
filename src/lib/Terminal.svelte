<script lang="ts">
  // One xterm.js terminal bound to one SSH connection.
  import { onMount } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { WebLinksAddon } from "@xterm/addon-web-links";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import "@xterm/xterm/css/xterm.css";
  import { api, type TerminalSettings } from "./api";
  import { attach } from "./bus";

  let {
    connId,
    settings,
    active,
    status,
  }: {
    connId: string | null;
    settings: TerminalSettings;
    active: boolean;
    status: string;
  } = $props();

  let host: HTMLDivElement;
  let term: Terminal | undefined;
  let fit: FitAddon | undefined;
  let attachedTo: string | null = null;
  let detach: (() => void) | undefined;
  let lastSize = "";

  function sendSize() {
    if (!term || !connId) return;
    const size = `${term.cols}x${term.rows}`;
    if (size === lastSize) return;
    lastSize = size;
    api.sshResize(connId, term.cols, term.rows).catch(() => {});
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

  onMount(() => {
    term = new Terminal({
      fontFamily: settings.font_family,
      fontSize: settings.font_size,
      scrollback: settings.scrollback,
      cursorBlink: true,
      allowProposedApi: true,
      theme: {
        background: "#1b1d23",
        foreground: "#d7dae0",
        cursor: "#61afef",
        selectionBackground: "#3e4451",
      },
    });
    fit = new FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new WebLinksAddon((_e, uri) => openUrl(uri)));
    term.open(host);

    // keyboard -> server
    term.onData((d) => {
      if (connId) api.sshWrite(connId, d).catch(() => {});
    });

    // Copy on Ctrl+Shift+C, paste on Ctrl+Shift+V (Ctrl+C stays "interrupt").
    term.attachCustomKeyEventHandler((ev) => {
      if (ev.type !== "keydown" || !ev.ctrlKey || !ev.shiftKey) return true;
      if (ev.code === "KeyC") {
        const sel = term!.getSelection();
        if (sel) navigator.clipboard.writeText(sel);
        return false;
      }
      if (ev.code === "KeyV") {
        navigator.clipboard.readText().then((t) => t && term!.paste(t));
        return false;
      }
      return true;
    });

    const ro = new ResizeObserver(() => refit());
    ro.observe(host);
    refit();

    return () => {
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

<div class="term" bind:this={host}></div>

<style>
  .term {
    position: absolute;
    inset: 0;
    padding: 4px 0 0 6px;
    background: #1b1d23;
  }
  .term :global(.xterm) {
    height: 100%;
  }
  .term :global(.xterm-viewport) {
    background-color: #1b1d23 !important;
  }
</style>
