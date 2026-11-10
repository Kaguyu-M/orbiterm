<script lang="ts">
  import { onMount } from "svelte";
  import Modal from "./Modal.svelte";
  import Icon from "./Icon.svelte";
  import { app } from "./app.svelte";
  import { api, asApiError, type SocketInfo } from "./api";

  let rows: SocketInfo[] = $state([]);
  let loading = $state(false);
  let error = $state("");
  let filter = $state("");
  let listenOnly = $state(true);
  let sortKey: keyof SocketInfo = $state("local_port");
  let asc = $state(true);

  async function refresh() {
    loading = true;
    error = "";
    try {
      rows = await api.openPorts();
    } catch (e) {
      error = asApiError(e).message;
    } finally {
      loading = false;
    }
  }
  onMount(refresh);

  const shown = $derived.by(() => {
    const f = filter.trim().toLowerCase();
    const list = rows.filter(
      (r) =>
        (!listenOnly || r.state === "ÉCOUTE") &&
        (!f || `${r.proto} ${r.local}:${r.local_port} ${r.remote} ${r.state} ${r.pid} ${r.process}`.toLowerCase().includes(f)),
    );
    const k = sortKey;
    return list.sort((a, b) => {
      const x = a[k], y = b[k];
      const c = typeof x === "number" && typeof y === "number" ? x - y : String(x).localeCompare(String(y));
      return asc ? c : -c;
    });
  });

  function sortBy(k: keyof SocketInfo) {
    if (sortKey === k) asc = !asc;
    else {
      sortKey = k;
      asc = true;
    }
  }
  const COLS: Array<[keyof SocketInfo, string]> = [
    ["proto", "Protocole"],
    ["local", "Adresse locale"],
    ["local_port", "Port"],
    ["remote", "Adresse distante"],
    ["state", "État"],
    ["pid", "PID"],
    ["process", "Programme"],
  ];
</script>

<Modal title="Ports ouverts et connexions de ce poste" onclose={() => (app.netTool = null)} width="860px">
  <div class="bar">
    <button onclick={refresh} disabled={loading}><Icon name="refresh" size={16} /> Actualiser</button>
    <label class="chk"><input type="checkbox" bind:checked={listenOnly} /> Ports en écoute uniquement</label>
    <span class="grow"></span>
    <input class="filter" bind:value={filter} placeholder="Filtrer (port, programme…)" aria-label="Filtrer" />
  </div>
  {#if error}<p class="error">{error}</p>{/if}
  <div class="tbl" data-open-ports>
    <table>
      <thead>
        <tr>
          {#each COLS as [k, l] (k)}
            <th><button class="th" onclick={() => sortBy(k)}>{l}{sortKey === k ? (asc ? " ▲" : " ▼") : ""}</button></th>
          {/each}
        </tr>
      </thead>
      <tbody>
        {#each shown as r, i (i)}
          <tr>
            <td>{r.proto}</td>
            <td class="mono">{r.local}</td>
            <td class="mono b">{r.local_port}</td>
            <td class="mono">{r.remote}</td>
            <td>{r.state}</td>
            <td class="mono">{r.pid || ""}</td>
            <td>{r.process}</td>
          </tr>
        {:else}
          <tr><td colspan="7" class="empty">{loading ? "Lecture…" : "Rien à afficher."}</td></tr>
        {/each}
      </tbody>
    </table>
  </div>
  <p class="muted">{shown.length} ligne(s) affichée(s) sur {rows.length}.</p>

  {#snippet actions()}
    <button class="primary dlg" onclick={() => (app.netTool = null)}>Fermer</button>
  {/snippet}
</Modal>

<style>
  .bar {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 6px;
  }
  .chk {
    display: flex;
    align-items: center;
    gap: 5px;
  }
  .grow {
    flex: 1;
  }
  .filter {
    width: 220px;
  }
  .tbl {
    height: 380px;
    overflow: auto;
    border: 1px solid var(--chrome-border);
  }
  table {
    width: 100%;
    border-collapse: collapse;
  }
  th {
    position: sticky;
    top: 0;
    background: var(--chrome);
    border-bottom: 1px solid var(--chrome-border);
    padding: 0;
    text-align: left;
  }
  .th {
    width: 100%;
    border: none;
    border-radius: 0;
    background: none;
    text-align: left;
    padding: 4px 6px;
    font-weight: normal;
    color: var(--text);
  }
  td {
    padding: 2px 6px;
    border-bottom: 1px solid var(--row-border);
    white-space: nowrap;
  }
  .mono {
    font-family: var(--mono);
    font-size: 12px;
  }
  .b {
    font-weight: 600;
  }
  .empty {
    color: var(--muted);
    text-align: center;
    padding: 20px;
  }
  .muted {
    color: var(--muted);
    margin: 6px 0 0;
  }
  .error {
    color: var(--danger);
  }
  .dlg {
    min-width: 90px;
    justify-content: center;
  }
</style>
