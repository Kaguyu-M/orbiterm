<script lang="ts">
  import Modal from "./Modal.svelte";
  import Icon from "./Icon.svelte";
  import { app } from "./app.svelte";
  import { api, asApiError } from "./api";

  type Target = { name: string; mac: string; broadcast: string; port: number };
  const KEY = "orbiterm.wol";
  function loadList(): Target[] {
    try {
      return JSON.parse(localStorage.getItem(KEY) ?? "[]");
    } catch {
      return [];
    }
  }
  let saved: Target[] = $state(loadList());
  let name = $state("");
  let mac = $state("");
  let broadcast = $state("255.255.255.255");
  let port = $state(9);
  let msg = $state("");
  let error = $state("");

  const MAC_RE = /^([0-9a-f]{2}[:\-. ]?){5}[0-9a-f]{2}$/i;
  const valid = $derived(MAC_RE.test(mac.trim()));

  function persist() {
    try {
      localStorage.setItem(KEY, JSON.stringify(saved));
    } catch {
      /* private mode */
    }
  }

  async function send(t?: Target) {
    const target = t ?? { name: name.trim(), mac: mac.trim(), broadcast: broadcast.trim(), port };
    msg = "";
    error = "";
    try {
      await api.wolSend(target.mac, target.broadcast, target.port);
      msg = `Paquet magique envoyé à ${target.name || target.mac} (${target.broadcast || "255.255.255.255"}:${target.port}).`;
      // remember the machine (most recent first)
      saved = [target, ...saved.filter((s) => s.mac.toLowerCase() !== target.mac.toLowerCase())].slice(0, 20);
      persist();
    } catch (e) {
      error = asApiError(e).message;
    }
  }

  function use(t: Target) {
    name = t.name;
    mac = t.mac;
    broadcast = t.broadcast;
    port = t.port;
  }
  function forget(t: Target) {
    saved = saved.filter((s) => s !== t);
    persist();
  }
</script>

<Modal title="Wake-on-LAN" onclose={() => (app.netTool = null)} width="560px">
  <p class="muted">Réveille une machine éteinte du réseau local en lui envoyant le « paquet magique ». Sa carte réseau doit avoir le réveil activé (BIOS/UEFI).</p>
  <div class="form">
    <label for="w-n">Nom (facultatif)</label>
    <input id="w-n" bind:value={name} placeholder="PC du bureau" />
    <label for="w-m">Adresse MAC</label>
    <input id="w-m" bind:value={mac} placeholder="00:11:22:33:44:55" spellcheck="false" class:bad={mac.trim() !== "" && !valid} />
    <label for="w-b">Diffusion</label>
    <span class="row">
      <input id="w-b" bind:value={broadcast} spellcheck="false" />
      <label for="w-p">Port</label>
      <input id="w-p" class="port" type="number" min="1" max="65535" bind:value={port} />
    </span>
  </div>
  {#if msg}<p class="ok">{msg}</p>{/if}
  {#if error}<p class="error">{error}</p>{/if}

  {#if saved.length}
    <div class="lt">Machines récentes</div>
    <div class="list">
      {#each saved as t (t.mac)}
        <div class="it">
          <button class="link" onclick={() => use(t)} title="Remplir le formulaire">
            <b>{t.name || "(sans nom)"}</b> <span class="mono">{t.mac}</span>
          </button>
          <button class="small" onclick={() => send(t)} title="Réveiller"><Icon name="wol" size={14} /> Réveiller</button>
          <button class="small" onclick={() => forget(t)} title="Oublier" aria-label="Oublier"><Icon name="trash" size={14} /></button>
        </div>
      {/each}
    </div>
  {/if}

  {#snippet actions()}
    <button class="primary dlg" disabled={!valid} onclick={() => send()} data-wol-send><Icon name="wol" size={16} /> Réveiller</button>
    <button class="dlg" onclick={() => (app.netTool = null)}>Fermer</button>
  {/snippet}
</Modal>

<style>
  .form {
    display: grid;
    grid-template-columns: 120px 1fr;
    align-items: center;
    gap: 6px 8px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .row input:first-child {
    flex: 1;
  }
  .port {
    width: 70px;
  }
  .bad {
    border-color: var(--danger);
  }
  .muted {
    color: var(--muted);
    margin: 0 0 10px;
  }
  .ok {
    color: #1f7035;
  }
  .error {
    color: var(--danger);
  }
  .lt {
    margin-top: 12px;
    font-weight: 600;
  }
  .list {
    border: 1px solid var(--chrome-border);
    max-height: 150px;
    overflow: auto;
    margin-top: 4px;
  }
  .it {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 2px 4px;
    border-bottom: 1px solid #f0f0f0;
  }
  .link {
    flex: 1;
    text-align: left;
    border: none;
    background: none;
    color: var(--text);
  }
  .mono {
    font-family: var(--mono);
    font-size: 12px;
    color: var(--muted);
  }
  .small {
    padding: 1px 6px;
    font-size: 11.5px;
  }
  .dlg {
    min-width: 90px;
    justify-content: center;
  }
</style>
