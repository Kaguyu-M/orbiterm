<script lang="ts">
  import Icon from "./Icon.svelte";
  import Logo from "./Logo.svelte";
  import { app } from "./app.svelte";
  import { describe, protocol } from "./api";

  let query = $state("");

  const results = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (!q) return [];
    return app.store.sessions
      .filter((s) => `${s.name} ${s.host} ${s.username}`.toLowerCase().includes(q))
      .slice(0, 8);
  });

  function submit(e: SubmitEvent) {
    e.preventDefault();
    if (results.length) app.openSession(results[0]);
    else if (query.trim()) app.quickConnect(query);
  }

  function ago(t: number) {
    const m = Math.round((Date.now() - t) / 60000);
    if (m < 1) return "à l'instant";
    if (m < 60) return `il y a ${m} min`;
    const h = Math.round(m / 60);
    if (h < 24) return `il y a ${h} h`;
    return new Date(t).toLocaleDateString();
  }
</script>

<div class="home">
  <div class="card">
    <div class="brand">
      <Logo size={52} />
      <div>
        <h1>Orbiterm</h1>
        <p class="muted">Terminal, SSH, SFTP et outils réseau — Windows &amp; Linux</p>
      </div>
    </div>

    <div class="big">
      <button class="start" data-home="local" onclick={() => app.startLocalTerminal()}>
        <Icon name="terminal" size={34} />
        <span>
          <b>Démarrer un terminal local</b>
          <small>{app.shells[0]?.label ?? "shell du système"}</small>
        </span>
      </button>
      <button class="start" data-home="new" onclick={() => app.newSession()}>
        <Icon name="session" size={34} />
        <span>
          <b>Nouvelle session</b>
          <small>SSH, Telnet, série, WSL…</small>
        </span>
      </button>
    </div>

    <form class="search" onsubmit={submit}>
      <Icon name="search" size={18} />
      <input
        bind:value={query}
        placeholder="Rechercher une session enregistrée, ou saisir un serveur (utilisateur@hôte)"
        spellcheck="false"
      />
    </form>
    {#if results.length}
      <div class="list">
        {#each results as s (s.id)}
          <button class="rec" ondblclick={() => app.openSession(s)} onclick={() => app.openSession(s)}>
            <Icon name={protocol(s.kind).icon} size={18} />
            <span class="n">{s.name}</span>
            <span class="d">{describe(s)}</span>
          </button>
        {/each}
      </div>
    {/if}

    <h2>Sessions récentes</h2>
    <div class="list">
      {#each app.recent as r (r.at)}
        <button class="rec" onclick={() => app.openSession(r.session)} title={describe(r.session)}>
          <Icon name={protocol(r.session.kind).icon} size={18} />
          <span class="n">{r.session.name}</span>
          <span class="d">{describe(r.session)}</span>
          <span class="t">{ago(r.at)}</span>
        </button>
      {:else}
        <p class="muted">Aucune session récente.</p>
      {/each}
    </div>
  </div>
</div>

<style>
  .home {
    position: absolute;
    inset: 0;
    overflow: auto;
    background: var(--home-bg);
    display: flex;
    justify-content: center;
    padding: 40px 16px;
    box-sizing: border-box;
  }
  .card {
    width: 640px;
    max-width: 100%;
    height: fit-content;
    background: var(--panel);
    border: 1px solid var(--chrome-border);
    box-shadow: 0 2px 10px var(--shadow);
    padding: 22px 26px 26px;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 14px;
    margin-bottom: 20px;
  }
  h1 {
    margin: 0;
    font-size: 26px;
    font-weight: 600;
    color: var(--brand);
  }
  .brand p {
    margin: 2px 0 0;
  }
  .big {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
    margin-bottom: 16px;
  }
  .start {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 14px;
    text-align: left;
    background: var(--chrome);
    border: 1px solid var(--chrome-border);
    border-radius: 3px;
  }
  .start:hover {
    background: var(--hover);
    border-color: var(--hover-border);
  }
  .start span {
    display: flex;
    flex-direction: column;
  }
  .start b {
    font-size: 14px;
  }
  .start small {
    color: var(--muted);
  }
  .search {
    display: flex;
    align-items: center;
    gap: 8px;
    border: 1px solid var(--chrome-border-dark);
    background: var(--input-bg);
    padding: 4px 8px;
  }
  .search input {
    flex: 1;
    border: none;
    outline: none;
    background: none;
    padding: 4px 0;
  }
  h2 {
    font-size: 13px;
    font-weight: 600;
    margin: 20px 0 6px;
    padding-bottom: 4px;
    border-bottom: 1px solid var(--chrome-border);
  }
  .list {
    display: flex;
    flex-direction: column;
  }
  .rec {
    display: flex;
    align-items: center;
    gap: 8px;
    border: 1px solid transparent;
    border-radius: 0;
    background: none;
    text-align: left;
    padding: 4px 6px;
  }
  .rec:hover {
    background: var(--hover);
    border-color: var(--hover-border);
  }
  .n {
    font-weight: 600;
    white-space: nowrap;
  }
  .d {
    flex: 1;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .t {
    color: var(--muted);
    font-size: 11.5px;
    white-space: nowrap;
  }
</style>
