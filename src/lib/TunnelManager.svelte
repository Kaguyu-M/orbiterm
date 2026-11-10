<script lang="ts">
  import Modal from "./Modal.svelte";
  import Icon from "./Icon.svelte";
  import { app } from "./app.svelte";
  import { forwardLabel, newForward, type ForwardKind, type Tunnel } from "./api";

  let edit: Tunnel | null = $state(null);
  let error = $state("");

  const sshSessions = $derived(app.store.sessions.filter((s) => s.kind === "ssh"));
  const sessionName = (id: string) => app.store.sessions.find((s) => s.id === id)?.name ?? "(session supprimée)";
  const kindLabel: Record<ForwardKind, string> = { local: "Local (-L)", remote: "Distant (-R)", dynamic: "Dynamique SOCKS (-D)" };

  function create() {
    error = "";
    edit = { id: "", name: "", session_id: sshSessions[0]?.id ?? "", forward: newForward("local"), autostart: false };
  }

  function setKind(k: ForwardKind) {
    if (!edit) return;
    const f = newForward(k);
    edit.forward = { ...f, bind_host: edit.forward.bind_host };
  }

  async function save() {
    if (!edit) return;
    error = "";
    const f = edit.forward;
    if (!edit.session_id) return (error = "Choisis la session SSH qui sert de serveur.");
    if (f.kind !== "remote" && (!f.bind_port || f.bind_port < 1)) return (error = "Port d'écoute invalide.");
    if (f.kind !== "dynamic" && (!f.dest_host.trim() || !f.dest_port)) return (error = "Destination incomplète.");
    if (!edit.name.trim()) edit.name = forwardLabel(f);
    await app.saveTunnel($state.snapshot(edit) as Tunnel);
    edit = null;
  }

  async function startAll() {
    for (const t of app.tunnels.filter((x) => !x.running)) await app.startTunnel(t);
  }
  async function stopAll() {
    for (const t of app.tunnels.filter((x) => x.running)) await app.stopTunnel(t);
  }
  async function toggleAuto(t: Tunnel) {
    await app.saveTunnel({ ...($state.snapshot(t) as Tunnel), autostart: !t.autostart });
  }
</script>

<Modal title="Gestionnaire de tunnels SSH" onclose={() => (app.showTunnels = false)} width="900px">
  {#if !edit}
    <div class="bar">
      <button onclick={create} disabled={!sshSessions.length} title={sshSessions.length ? "" : "Crée d'abord une session SSH"}>
        <Icon name="tunnel" size={18} /> Nouveau tunnel SSH
      </button>
      <button onclick={startAll} disabled={!app.tunnels.some((t) => !t.running)}><Icon name="play" size={16} /> Tout démarrer</button>
      <button onclick={stopAll} disabled={!app.tunnels.some((t) => t.running)}><Icon name="stop" size={16} /> Tout arrêter</button>
      <button onclick={() => app.refreshTunnels()}><Icon name="refresh" size={16} /> Actualiser</button>
    </div>
    <table class="list">
      <thead>
        <tr><th></th><th>Nom</th><th>Type</th><th>Écoute</th><th>Serveur SSH</th><th>Destination</th><th>Auto</th><th></th></tr>
      </thead>
      <tbody>
        {#each app.tunnels as t (t.id)}
          <tr class:on={t.running} data-tunnel={t.name}>
            <td><span class="led" class:run={t.running} title={t.running ? "Actif" : "Arrêté"}></span></td>
            <td class="nm">{t.name}</td>
            <td>{kindLabel[t.forward.kind]}</td>
            <td class="mono">{t.forward.bind_host || (t.forward.kind === "remote" ? "serveur" : "127.0.0.1")}:{t.running && t.port ? t.port : t.forward.bind_port}</td>
            <td>{sessionName(t.session_id)}</td>
            <td class="mono">{t.forward.kind === "dynamic" ? "(SOCKS)" : `${t.forward.dest_host}:${t.forward.dest_port}`}</td>
            <td><input type="checkbox" checked={t.autostart} onchange={() => toggleAuto(t)} aria-label="Démarrage automatique" /></td>
            <td class="acts">
              {#if t.running}
                <button title="Arrêter" aria-label="Arrêter" onclick={() => app.stopTunnel(t)}><Icon name="stop" size={14} /></button>
              {:else}
                <button title="Démarrer" aria-label="Démarrer" onclick={() => app.startTunnel(t)}><Icon name="play" size={14} /></button>
              {/if}
              <button title="Modifier" aria-label="Modifier" onclick={() => { error = ""; edit = structuredClone($state.snapshot(t)) as Tunnel; }}><Icon name="edit" size={14} /></button>
              <button title="Supprimer" aria-label="Supprimer" onclick={() => app.deleteTunnel(t)}><Icon name="trash" size={14} /></button>
            </td>
          </tr>
        {:else}
          <tr><td colspan="8" class="empty">Aucun tunnel. Clique sur « Nouveau tunnel SSH ».</td></tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <div class="kinds" role="radiogroup" aria-label="Type de tunnel">
      {#each ["local", "remote", "dynamic"] as const as k (k)}
        <label class="kind" class:on={edit.forward.kind === k}>
          <input type="radio" name="kind" checked={edit.forward.kind === k} onchange={() => setKind(k)} />
          {kindLabel[k]}
        </label>
      {/each}
    </div>

    <!-- diagram: my computer -> SSH server -> destination -->
    <div class="diagram">
      <div class="box">
        <div class="bt"><Icon name="session" size={30} /> Mon ordinateur</div>
        {#if edit.forward.kind === "remote"}
          <label>Destination (vue d'ici)
            <span class="pair"><input bind:value={edit.forward.dest_host} placeholder="127.0.0.1" /><input class="port" type="number" min="1" max="65535" bind:value={edit.forward.dest_port} aria-label="Port de destination" /></span>
          </label>
        {:else}
          <label>Port d'écoute local
            <span class="pair"><input bind:value={edit.forward.bind_host} placeholder="127.0.0.1" /><input class="port" type="number" min="1" max="65535" bind:value={edit.forward.bind_port} aria-label="Port d'écoute" /></span>
          </label>
        {/if}
      </div>
      <div class="arrow">{edit.forward.kind === "remote" ? "⟵" : "⟶"}<small>chiffré SSH</small></div>
      <div class="box">
        <div class="bt"><Icon name="ssh" size={30} /> Serveur SSH</div>
        <label>Session SSH
          <select bind:value={edit.session_id}>
            {#each sshSessions as s (s.id)}<option value={s.id}>{s.name}</option>{/each}
          </select>
        </label>
        {#if edit.forward.kind === "remote"}
          <label>Port ouvert sur le serveur
            <span class="pair"><input bind:value={edit.forward.bind_host} placeholder="localhost" /><input class="port" type="number" min="0" max="65535" bind:value={edit.forward.bind_port} aria-label="Port distant" /></span>
          </label>
        {/if}
      </div>
      <div class="arrow">{edit.forward.kind === "remote" ? "⟵" : "⟶"}</div>
      <div class="box">
        <div class="bt"><Icon name={edit.forward.kind === "dynamic" ? "browser" : "servers"} size={30} /> {edit.forward.kind === "dynamic" ? "Internet / réseau" : edit.forward.kind === "remote" ? "Clients du serveur" : "Serveur de destination"}</div>
        {#if edit.forward.kind === "local"}
          <label>Hôte et port (vus du serveur SSH)
            <span class="pair"><input bind:value={edit.forward.dest_host} placeholder="db.interne" /><input class="port" type="number" min="1" max="65535" bind:value={edit.forward.dest_port} aria-label="Port de destination" /></span>
          </label>
        {:else if edit.forward.kind === "dynamic"}
          <p class="muted">Configure ton navigateur ou ton application avec le proxy SOCKS 127.0.0.1:{edit.forward.bind_port}.</p>
        {:else}
          <p class="muted">Les connexions au port ouvert sur le serveur arrivent ici.</p>
        {/if}
      </div>
    </div>

    <div class="line">
      <label for="tn">Nom du tunnel</label>
      <input id="tn" bind:value={edit.name} placeholder={forwardLabel(edit.forward)} />
      <label class="chk"><input type="checkbox" bind:checked={edit.autostart} /> Démarrer au lancement d'Orbiterm</label>
    </div>
    {#if error}<p class="error">{error}</p>{/if}
  {/if}

  {#snippet actions()}
    {#if edit}
      <button class="primary dlg" onclick={save}>Enregistrer</button>
      <button class="dlg" onclick={() => (edit = null)}>Annuler</button>
    {:else}
      <button class="primary dlg" onclick={() => (app.showTunnels = false)}>Fermer</button>
    {/if}
  {/snippet}
</Modal>

<style>
  .bar {
    display: flex;
    gap: 6px;
    margin-bottom: 8px;
  }
  .list {
    width: 100%;
    border-collapse: collapse;
    border: 1px solid var(--chrome-border);
  }
  .list th {
    text-align: left;
    font-weight: normal;
    background: var(--chrome);
    border-bottom: 1px solid var(--chrome-border);
    padding: 4px 6px;
  }
  .list td {
    padding: 3px 6px;
    border-bottom: 1px solid var(--row-border);
  }
  .list tr.on td {
    background: var(--ok-bg);
  }
  .nm {
    font-weight: 600;
  }
  .mono {
    font-family: var(--mono);
    font-size: 12px;
  }
  .acts {
    white-space: nowrap;
  }
  .acts button {
    padding: 2px 4px;
  }
  .led {
    display: inline-block;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--switch-off);
    border: 1px solid var(--chrome-border-dark);
  }
  .led.run {
    background: var(--ok);
    border-color: var(--ok);
  }
  .empty {
    color: var(--muted);
    text-align: center;
    padding: 18px;
  }
  .kinds {
    display: flex;
    gap: 6px;
    margin-bottom: 10px;
  }
  .kind {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 5px 10px;
    border: 1px solid var(--chrome-border);
    border-radius: 3px;
  }
  .kind.on {
    background: var(--selected);
    border-color: var(--accent);
  }
  .diagram {
    display: grid;
    grid-template-columns: 1fr 70px 1fr 50px 1fr;
    align-items: stretch;
    gap: 4px;
    margin-bottom: 12px;
  }
  .box {
    border: 1px solid var(--chrome-border-dark);
    background: var(--chrome);
    padding: 8px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .bt {
    display: flex;
    align-items: center;
    gap: 6px;
    font-weight: 600;
  }
  .box label {
    display: flex;
    flex-direction: column;
    gap: 3px;
    color: var(--muted);
  }
  .box select {
    width: 100%;
  }
  .pair {
    display: flex;
    gap: 4px;
  }
  .pair input:not(.port) {
    flex: 1;
    min-width: 60px;
  }
  .port {
    width: 70px;
  }
  .arrow {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    font-size: 26px;
    color: var(--accent);
  }
  .arrow small {
    font-size: 10px;
    color: var(--muted);
  }
  .line {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .line input:not([type="checkbox"]) {
    flex: 1;
  }
  .chk {
    display: flex;
    align-items: center;
    gap: 5px;
    white-space: nowrap;
  }
  .error {
    color: var(--danger);
    margin: 8px 0 0;
  }
  .dlg {
    min-width: 90px;
    justify-content: center;
  }
</style>
