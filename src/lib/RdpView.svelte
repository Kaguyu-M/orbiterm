<script lang="ts" module>
  /** KeyboardEvent.code → scancode set 1 (0xE0xx = extended). Physical keys: the remote layout applies. */
  const SC: Record<string, number> = {
    Escape: 0x01, Digit1: 0x02, Digit2: 0x03, Digit3: 0x04, Digit4: 0x05, Digit5: 0x06, Digit6: 0x07, Digit7: 0x08,
    Digit8: 0x09, Digit9: 0x0a, Digit0: 0x0b, Minus: 0x0c, Equal: 0x0d, Backspace: 0x0e, Tab: 0x0f,
    KeyQ: 0x10, KeyW: 0x11, KeyE: 0x12, KeyR: 0x13, KeyT: 0x14, KeyY: 0x15, KeyU: 0x16, KeyI: 0x17, KeyO: 0x18,
    KeyP: 0x19, BracketLeft: 0x1a, BracketRight: 0x1b, Enter: 0x1c, ControlLeft: 0x1d,
    KeyA: 0x1e, KeyS: 0x1f, KeyD: 0x20, KeyF: 0x21, KeyG: 0x22, KeyH: 0x23, KeyJ: 0x24, KeyK: 0x25, KeyL: 0x26,
    Semicolon: 0x27, Quote: 0x28, Backquote: 0x29, ShiftLeft: 0x2a, Backslash: 0x2b,
    KeyZ: 0x2c, KeyX: 0x2d, KeyC: 0x2e, KeyV: 0x2f, KeyB: 0x30, KeyN: 0x31, KeyM: 0x32, Comma: 0x33, Period: 0x34,
    Slash: 0x35, ShiftRight: 0x36, NumpadMultiply: 0x37, AltLeft: 0x38, Space: 0x39, CapsLock: 0x3a,
    F1: 0x3b, F2: 0x3c, F3: 0x3d, F4: 0x3e, F5: 0x3f, F6: 0x40, F7: 0x41, F8: 0x42, F9: 0x43, F10: 0x44,
    NumLock: 0x45, ScrollLock: 0x46, Numpad7: 0x47, Numpad8: 0x48, Numpad9: 0x49, NumpadSubtract: 0x4a,
    Numpad4: 0x4b, Numpad5: 0x4c, Numpad6: 0x4d, NumpadAdd: 0x4e, Numpad1: 0x4f, Numpad2: 0x50, Numpad3: 0x51,
    Numpad0: 0x52, NumpadDecimal: 0x53, IntlBackslash: 0x56, F11: 0x57, F12: 0x58, IntlRo: 0x73, IntlYen: 0x7d,
    NumpadEnter: 0xe01c, ControlRight: 0xe01d, NumpadDivide: 0xe035, PrintScreen: 0xe037, AltRight: 0xe038,
    Home: 0xe047, ArrowUp: 0xe048, PageUp: 0xe049, ArrowLeft: 0xe04b, ArrowRight: 0xe04d, End: 0xe04f,
    ArrowDown: 0xe050, PageDown: 0xe051, Insert: 0xe052, Delete: 0xe053, MetaLeft: 0xe05b, MetaRight: 0xe05c,
    ContextMenu: 0xe05d,
  };
</script>

<script lang="ts">
  // Embedded RDP session: the remote desktop drawn in a canvas, fed by the backend (IronRDP).
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import Icon from "./Icon.svelte";
  import { api } from "./api";
  import { app, type Tab } from "./app.svelte";

  let { tab, active }: { tab: Tab; active: boolean } = $props();

  let box: HTMLDivElement;
  let canvas: HTMLCanvasElement;
  let ctx: CanvasRenderingContext2D | null = null;
  let ws: WebSocket | null = null;
  let attachedTo: string | null = null;
  let desk = $state({ w: 0, h: 0 });
  let view = $state({ w: 0, h: 0 }); // canvas CSS size
  let connected = $state(false);
  let status = $state("");
  let full = $state(false);
  let focused = $state(false);
  const pressed = new Set<number>();
  let needSync = true;
  // svelte-ignore state_referenced_locally
  const fit = (tab.session.extra?.rdp_size || "fit") === "fit";

  function send(m: object) {
    if (ws?.readyState === WebSocket.OPEN) ws.send(JSON.stringify(m));
  }

  /** Canvas display size: remote pixels → physical pixels, never larger than the tab. */
  function layout() {
    if (!box || !desk.w) return;
    const dpr = window.devicePixelRatio || 1;
    const cw = box.clientWidth, ch = box.clientHeight;
    const s = Math.min(cw / (desk.w / dpr), ch / (desk.h / dpr), 1);
    view = { w: Math.floor((desk.w / dpr) * s), h: Math.floor((desk.h / dpr) * s) };
  }

  function requestFit() {
    if (!fit || !box || !connected) return;
    const dpr = window.devicePixelRatio || 1;
    const w = Math.round(box.clientWidth * dpr), h = Math.round(box.clientHeight * dpr);
    if (w >= 200 && h >= 200 && (w !== desk.w || h !== desk.h)) send({ t: "r", w, h });
  }

  function onMessage(ev: MessageEvent) {
    const buf = ev.data as ArrayBuffer;
    const dv = new DataView(buf);
    const t = dv.getUint8(0);
    if (t === 1 && ctx) {
      const x = dv.getUint16(1, true), y = dv.getUint16(3, true), w = dv.getUint16(5, true), h = dv.getUint16(7, true);
      ctx.putImageData(new ImageData(new Uint8ClampedArray(buf, 9, w * h * 4), w, h), x, y);
    } else if (t === 2) {
      desk = { w: dv.getUint16(1, true), h: dv.getUint16(3, true) };
      canvas.width = desk.w;
      canvas.height = desk.h;
      ctx = canvas.getContext("2d", { alpha: false });
      layout();
    } else if (t === 3) {
      const hx = dv.getUint16(1, true), hy = dv.getUint16(3, true), w = dv.getUint16(5, true), h = dv.getUint16(7, true);
      if (!w || !h) return;
      const c = document.createElement("canvas");
      c.width = w;
      c.height = h;
      c.getContext("2d")!.putImageData(new ImageData(new Uint8ClampedArray(buf.slice(9, 9 + w * h * 4)), w, h), 0, 0);
      canvas.style.cursor = `url(${c.toDataURL()}) ${hx} ${hy}, default`;
    } else if (t === 4) {
      canvas.style.cursor = "default";
    } else if (t === 5) {
      canvas.style.cursor = "none";
    }
  }

  async function attach(connId: string) {
    ws?.close();
    connected = false;
    status = "Ouverture du bureau à distance…";
    const v = await api.rdpUrl(connId);
    desk = { w: v.width, h: v.height };
    const s = new WebSocket(v.url);
    s.binaryType = "arraybuffer";
    ws = s;
    s.onopen = () => {
      connected = true;
      status = "";
      if (active) canvas.focus();
      setTimeout(requestFit, 300);
    };
    s.onmessage = onMessage;
    s.onclose = () => {
      if (ws !== s) return;
      connected = false;
      status = "Bureau à distance déconnecté.";
    };
  }

  $effect(() => {
    const id = tab.connId;
    if (id && id !== attachedTo && canvas) {
      attachedTo = id;
      attach(id).catch((e) => (status = `Échec : ${e?.message ?? e}`));
    }
  });

  $effect(() => {
    if (active && connected) {
      layout();
      requestFit();
      canvas?.focus();
    }
  });

  onMount(() => {
    const ro = new ResizeObserver(() => {
      layout();
      requestFit(); // the backend waits until the size stops changing
    });
    ro.observe(box);
    return () => {
      ro.disconnect();
      ws?.close();
    };
  });

  // --------------------------------------------------------- input
  function pos(e: MouseEvent) {
    const r = canvas.getBoundingClientRect();
    const x = Math.round(((e.clientX - r.left) * desk.w) / r.width);
    const y = Math.round(((e.clientY - r.top) * desk.h) / r.height);
    return { x: Math.max(0, Math.min(desk.w - 1, x)), y: Math.max(0, Math.min(desk.h - 1, y)) };
  }
  let moveQueued: { x: number; y: number } | null = null;
  function onMove(e: PointerEvent) {
    const first = !moveQueued;
    moveQueued = pos(e);
    if (first)
      requestAnimationFrame(() => {
        if (moveQueued) send({ t: "m", ...moveQueued });
        moveQueued = null;
      });
  }
  function onButton(e: PointerEvent, down: boolean) {
    if (down) {
      canvas.focus();
      canvas.setPointerCapture(e.pointerId);
    }
    const b = e.button === 1 ? 1 : e.button === 2 ? 2 : 0;
    send({ t: "b", b, down, ...pos(e) });
    e.preventDefault();
  }
  function onWheel(e: WheelEvent) {
    e.preventDefault();
    const k = e.deltaMode === 1 ? 40 : e.deltaMode === 2 ? 800 : 1;
    const d = Math.max(-255, Math.min(255, Math.round((-e.deltaY * k * 120) / 100)));
    if (d) send({ t: "w", d, h: false, ...pos(e) });
  }
  function onKey(e: KeyboardEvent, up: boolean) {
    const sc = SC[e.code];
    if (sc === undefined) return;
    e.preventDefault();
    e.stopPropagation();
    if (needSync) {
      needSync = false;
      send({ t: "s", caps: e.getModifierState("CapsLock"), num: e.getModifierState("NumLock"), scroll: e.getModifierState("ScrollLock") });
    }
    if (up) pressed.delete(sc);
    else pressed.add(sc);
    send({ t: "k", sc, up });
  }
  function releaseAll() {
    for (const sc of pressed) send({ t: "k", sc, up: true });
    pressed.clear();
    needSync = true;
  }

  async function pasteText() {
    const text = await navigator.clipboard.readText().catch(() => "");
    if (text) send({ t: "u", s: text });
    canvas.focus();
  }

  async function toggleFull() {
    full = !full;
    await getCurrentWindow().setFullscreen(full).catch(() => {});
    setTimeout(() => {
      layout();
      requestFit();
      canvas.focus();
    }, 250);
  }
</script>

<div class="rdp" class:full>
  <div class="bar">
    <Icon name="rdp" size={16} />
    <span class="title">{tab.title}</span>
    <span class="dim">{desk.w ? `${desk.w}×${desk.h}` : ""}</span>
    <button title="Envoyer Ctrl+Alt+Suppr" disabled={!connected} onclick={() => { send({ t: "cad" }); canvas.focus(); }}>Ctrl+Alt+Suppr</button>
    <button title="Taper le texte du presse-papiers local dans la session" disabled={!connected} onclick={pasteText}>
      <Icon name="copy" size={14} /> Coller le texte
    </button>
    <button title={full ? "Quitter le plein écran" : "Plein écran"} disabled={!connected} onclick={toggleFull}>
      {full ? "Quitter le plein écran" : "Plein écran"}
    </button>
    <span class="state" class:ok={connected}>
      {#if connected}● connecté{#if focused}&nbsp;— clavier capturé{/if}{:else}{status}{/if}
    </span>
  </div>
  <div class="screen" bind:this={box} data-rdp>
    <canvas
      bind:this={canvas}
      tabindex="0"
      style="width:{view.w}px;height:{view.h}px"
      onpointermove={onMove}
      onpointerdown={(e) => onButton(e, true)}
      onpointerup={(e) => onButton(e, false)}
      onwheel={onWheel}
      oncontextmenu={(e) => e.preventDefault()}
      onkeydown={(e) => onKey(e, false)}
      onkeyup={(e) => onKey(e, true)}
      onfocus={() => (focused = true)}
      onblur={() => {
        focused = false;
        releaseAll();
      }}
    ></canvas>
    {#if !connected && status}
      <div class="msg">{status}</div>
    {/if}
  </div>
</div>

<style>
  .rdp {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    background: #0f1115;
  }
  .rdp.full {
    position: fixed;
    z-index: 900;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 3px 8px;
    background: var(--chrome);
    border-bottom: 1px solid var(--chrome-border);
  }
  .full .bar {
    position: absolute;
    top: 0;
    left: 50%;
    /* a 5 px strip stays visible: hovering it reveals the bar */
    transform: translate(-50%, calc(-100% + 5px));
    z-index: 2;
    border: 1px solid var(--chrome-border);
    border-top: none;
    padding-top: 6px;
    transition: transform 0.15s;
  }
  .full .bar::after {
    content: "";
    position: absolute;
    left: 0;
    right: 0;
    bottom: -10px;
    height: 10px;
  }
  .full .bar:hover {
    transform: translate(-50%, 0);
  }
  .bar button {
    padding: 1px 8px;
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .title {
    font-weight: 600;
  }
  .dim {
    color: var(--muted);
    margin-right: 6px;
  }
  .state {
    margin-left: auto;
    color: var(--muted);
  }
  .state.ok {
    color: var(--ok);
  }
  .screen {
    flex: 1;
    min-height: 0;
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
  }
  canvas {
    display: block;
    outline: none;
    image-rendering: auto;
    touch-action: none;
  }
  .msg {
    position: absolute;
    top: 50%;
    left: 0;
    right: 0;
    text-align: center;
    color: #bbb;
  }
</style>
