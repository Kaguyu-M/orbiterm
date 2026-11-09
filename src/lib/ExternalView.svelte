<script lang="ts">
  // Tab of a session shown in its own window (RDP with mstsc on Windows).
  import Icon from "./Icon.svelte";
  import { describe, protocol } from "./api";
  import { app, type Tab } from "./app.svelte";

  let { tab }: { tab: Tab } = $props();
  const p = $derived(protocol(tab.session.kind));
</script>

<div class="ext">
  <div class="card">
    <Icon name={p.icon} size={56} />
    <h2>{tab.title}</h2>
    <p class="desc">{describe(tab.session)}</p>
    {#if tab.state === "connecting"}
      <p class="st">{tab.status || "Ouverture…"}</p>
    {:else if tab.state === "open"}
      <p class="st ok">● Bureau à distance ouvert dans sa propre fenêtre</p>
      <p class="muted">
        La connexion utilise le client Bureau à distance de Windows (mstsc) : il demande le mot de passe et peut le
        mémoriser. Fermer cet onglet ferme aussi la fenêtre distante.
      </p>
    {:else}
      <p class="st err">{tab.status || "Fenêtre fermée."}</p>
    {/if}
    <div class="acts">
      {#if tab.state !== "connecting"}
        <button class="primary" onclick={() => app.reconnect(tab)}><Icon name="refresh" size={14} /> {tab.state === "open" ? "Relancer" : "Rouvrir"}</button>
      {/if}
      <button onclick={() => app.closeTab(tab)}><Icon name="close" size={12} /> Fermer la session</button>
    </div>
  </div>
</div>

<style>
  .ext {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--home-bg);
  }
  .card {
    width: 460px;
    padding: 24px;
    background: var(--panel);
    border: 1px solid var(--chrome-border);
    text-align: center;
  }
  h2 {
    margin: 8px 0 2px;
    font-size: 17px;
  }
  .desc {
    color: var(--muted);
    margin: 0 0 14px;
  }
  .st {
    font-weight: 600;
  }
  .ok {
    color: var(--ok);
  }
  .err {
    color: var(--danger);
  }
  .muted {
    font-size: 12px;
  }
  .acts {
    display: flex;
    gap: 8px;
    justify-content: center;
    margin-top: 14px;
  }
</style>
