<script lang="ts">
  // VNC viewer (noVNC) connected to the backend WebSocket bridge of the tab.
  import RFB from "@novnc/novnc";
  import Icon from "./Icon.svelte";
  import { api, type Session } from "./api";
  import { app, type Tab } from "./app.svelte";

  let { tab, active }: { tab: Tab; active: boolean } = $props();

  let screen: HTMLDivElement;
  let rfb: RFB | null = null;
  let status = $state("");
  let connected = $state(false);
  // svelte-ignore state_referenced_locally
  let scale = $state(tab.session.extra?.vnc_scale !== "0");
  // svelte-ignore state_referenced_locally
  let viewOnly = $state(tab.session.extra?.vnc_viewonly === "1");
  let desktop = $state("");
  let attachedTo: string | null = null;

  const s = (): Session => tab.session;
  const saved = () => !!s().id && s().remember_password;

  async function start(connId: string) {
    rfb?.disconnect();
    connected = false;
    status = "Connexion au serveur VNC…";
    const url = await api.vncUrl(connId);
    let stored = saved() ? await api.vncPasswordGet(s().id).catch(() => null) : null;
    const r = new RFB(screen, url, { wsProtocols: ["binary"] });
    rfb = r;
    r.scaleViewport = scale;
    r.viewOnly = viewOnly;
    r.background = "#1e1e1e";
    r.addEventListener("connect", () => {
      connected = true;
      status = "";
      if (active) r.focus();
    });
    r.addEventListener("desktopname", (e) => (desktop = (e as CustomEvent).detail.name));
    r.addEventListener("credentialsrequired", async () => {
      if (stored) {
        r.sendCredentials({ password: stored });
        stored = null; // used once: a refusal asks again
        return;
      }
      const a = await app.ask(
        `Mot de passe VNC pour ${s().host}:${s().port}`,
        "",
        true,
        s().id ? "Mémoriser dans le trousseau du système" : undefined,
        saved(),
      );
      if (!a) {
        r.disconnect();
        return;
      }
      if (s().id && a.checked) await api.vncPasswordSet(s().id, a.value).catch(() => {});
      r.sendCredentials({ password: a.value });
    });
    r.addEventListener("securityfailure", (e) => {
      const reason = (e as CustomEvent).detail.reason || "authentification refusée";
      status = `Échec : ${reason}`;
      if (s().id) api.vncPasswordSet(s().id, null).catch(() => {});
    });
    r.addEventListener("disconnect", (e) => {
      connected = false;
      if (rfb !== r) return;
      if (!status.startsWith("Échec")) status = (e as CustomEvent).detail.clean ? "Déconnecté." : "Connexion perdue.";
      tab.state = status.startsWith("Échec") ? "error" : "closed";
    });
  }

  $effect(() => {
    const id = tab.connId;
    if (id && id !== attachedTo && screen) {
      attachedTo = id;
      start(id).catch((e) => (status = `Échec : ${e?.message ?? e}`));
    }
  });

  $effect(() => {
    if (rfb) {
      rfb.scaleViewport = scale;
      rfb.viewOnly = viewOnly;
    }
  });

  $effect(() => {
    if (active && connected) rfb?.focus();
  });

  $effect(() => () => rfb?.disconnect());

  async function pasteClipboard() {
    const text = await navigator.clipboard.readText().catch(() => "");
    if (text) rfb?.clipboardPasteFrom(text);
  }
</script>

<div class="vnc">
  <div class="bar">
    <Icon name="vnc" size={16} />
    <span class="title">{desktop || `${s().host}:${s().port}`}</span>
    <button title="Envoyer Ctrl+Alt+Suppr" disabled={!connected || viewOnly} onclick={() => rfb?.sendCtrlAltDel()}>Ctrl+Alt+Suppr</button>
    <button title="Envoyer le presse-papiers local au serveur" disabled={!connected} onclick={pasteClipboard}><Icon name="copy" size={14} /> Presse-papiers</button>
    <label class="chk"><input type="checkbox" bind:checked={scale} /> Ajuster</label>
    <label class="chk"><input type="checkbox" bind:checked={viewOnly} /> Lecture seule</label>
    <span class="state" class:ok={connected}>{connected ? "● connecté" : status}</span>
  </div>
  <div class="screen" bind:this={screen} data-vnc></div>
  {#if !connected && status}
    <div class="msg">{status}</div>
  {/if}
</div>

<style>
  .vnc {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    background: #1e1e1e;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 3px 8px;
    background: var(--chrome);
    border-bottom: 1px solid var(--chrome-border);
  }
  .bar button {
    padding: 1px 8px;
  }
  .title {
    font-weight: 600;
    margin-right: 6px;
  }
  .chk {
    display: flex;
    align-items: center;
    gap: 4px;
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
