<script lang="ts">
  // Browser session: a native child webview placed over the area below the address bar.
  // The remote page has no access to Orbiterm's commands (separate webview, remote origin).
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import Icon from "./Icon.svelte";
  import { api } from "./api";
  import { app, type Tab } from "./app.svelte";

  let { tab, visible }: { tab: Tab; visible: boolean } = $props();

  const label = $derived(`browser-${tab.key}`);
  let area: HTMLDivElement;
  // svelte-ignore state_referenced_locally
  let address = $state(tab.session.host);
  // svelte-ignore state_referenced_locally
  let current = $state(tab.session.host);
  let loading = $state(true);
  let error = $state("");
  let opened = $state(false);
  let rect = $state({ x: 0, y: 0, w: 0, h: 0 });

  function measure() {
    if (!area) return;
    const r = area.getBoundingClientRect();
    rect = { x: Math.round(r.left), y: Math.round(r.top), w: Math.round(r.width), h: Math.round(r.height) };
  }

  onMount(() => {
    const ro = new ResizeObserver(measure);
    ro.observe(area);
    window.addEventListener("resize", measure);
    const un = listen<{ label: string; url: string; loading: boolean }>("browser-nav", (e) => {
      if (e.payload.label !== label) return;
      loading = e.payload.loading;
      current = e.payload.url;
      if (document.activeElement?.id !== `addr-${tab.key}`) address = e.payload.url;
      if (!loading) {
        tab.state = "open";
        tab.status = "";
      }
    });
    measure();
    api
      .browserOpen(label, tab.session.host, rect.x, rect.y, rect.w, rect.h)
      .then((u) => {
        opened = true;
        current = address = u;
        tab.state = "open";
      })
      .catch((e) => {
        error = e?.message ?? String(e);
        tab.state = "error";
      });
    return () => {
      ro.disconnect();
      window.removeEventListener("resize", measure);
      un.then((f) => f());
    };
  });

  // keep the native webview aligned with the placeholder; hide it under menus/dialogs
  $effect(() => {
    const show = visible && !app.overlay;
    const { x, y, w, h } = rect;
    if (opened) api.browserBounds(label, x, y, w, h, show).catch(() => {});
  });

  async function go(e?: Event) {
    e?.preventDefault();
    error = "";
    try {
      current = address = await api.browserNavigate(label, address);
    } catch (err) {
      error = (err as { message?: string })?.message ?? String(err);
    }
  }
  const act = (a: "back" | "forward" | "reload" | "stop") => api.browserAction(label, a).catch(() => {});
</script>

<div class="br">
  <form class="bar" onsubmit={go}>
    <button type="button" title="Précédent" onclick={() => act("back")}><Icon name="back" size={14} /></button>
    <button type="button" title="Suivant" onclick={() => act("forward")}><Icon name="forward" size={14} /></button>
    {#if loading}
      <button type="button" title="Arrêter" onclick={() => act("stop")}><Icon name="close" size={12} /></button>
    {:else}
      <button type="button" title="Actualiser" onclick={() => act("reload")}><Icon name="refresh" size={14} /></button>
    {/if}
    <input id="addr-{tab.key}" class="addr" bind:value={address} spellcheck="false" data-browser-addr />
    <button type="submit" title="Aller">Aller</button>
    <button type="button" title="Ouvrir dans le navigateur du système" onclick={() => openUrl(current).catch(() => {})}>
      <Icon name="browser" size={14} />
    </button>
  </form>
  <div class="page" bind:this={area} data-browser-area>
    {#if error}
      <div class="msg err">{error}</div>
    {:else if !opened}
      <div class="msg">Chargement…</div>
    {/if}
  </div>
</div>

<style>
  .br {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    background: var(--panel);
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 3px 6px;
    background: var(--chrome);
    border-bottom: 1px solid var(--chrome-border);
  }
  .bar button {
    padding: 1px 6px;
    display: flex;
    align-items: center;
  }
  .addr {
    flex: 1;
    min-width: 0;
  }
  .page {
    flex: 1;
    min-height: 0;
    position: relative;
    background: #fff;
  }
  .msg {
    padding: 20px;
    color: var(--muted);
  }
  .err {
    color: var(--danger);
  }
</style>
