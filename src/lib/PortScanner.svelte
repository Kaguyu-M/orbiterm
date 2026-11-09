<script lang="ts">
  import { onDestroy } from "svelte";
  import Modal from "./Modal.svelte";
  import Icon from "./Icon.svelte";
  import { app } from "./app.svelte";
  import { api, asApiError, type ScanHit, type ScanState } from "./api";

  const SCAN_ID = "ui";
  const KEY = "orbiterm.scanner";
  type Saved = { targets: string; ports: string; timeout: number };
  function saved(): Saved | null {
    try {
      return JSON.parse(localStorage.getItem(KEY) ?? "null");
    } catch {
      return null;
    }
  }
  /** this computer's /24 network, from the address the servers report */
  function localNet(): string {
    const ip = app.servers.flatMap((s) => s.addresses).find((a) => a !== "127.0.0.1");
    return ip ? ip.replace(/\.\d+$/, ".0/24") : "192.168.1.0/24";
  }
  const init = saved();
  let targets = $state(init?.targets ?? localNet());
  let ports = $state(init?.ports ?? "communs");
  let timeout = $state(init?.timeout ?? 400);
  let scan = $state<ScanState | null>(null);
  let running = $state(false);
  let error = $state("");
  let timer: ReturnType<typeof setInterval> | undefined;

  const PRESETS: Array<[string, string]> = [
    ["Ports courants", "communs"],
    ["Administration (SSH, Telnet, RDP, VNC, WinRM)", "22,23,3389,5900-5903,5985,5986"],
    ["Web", "80,443,3000,5000,8000,8080,8443,8888,9000,9090"],
    ["Fichiers (FTP, SMB, NFS, rsync)", "21,139,445,2049,873"],
    ["Bases de données", "1433,1521,3306,5432,6379,9200,27017"],
    ["Ports 1 à 1024", "1-1024"],
  ];

  async function start() {
    error = "";
    try {
      localStorage.setItem(KEY, JSON.stringify({ targets, ports, timeout }));
    } catch {
      /* private mode */
    }
    try {
      await api.scanStart(SCAN_ID, targets, ports, timeout, 256);
      running = true;
      scan = await api.scanState(SCAN_ID);
      clearInterval(timer);
      timer = setInterval(poll, 250);
    } catch (e) {
      error = asApiError(e).message;
    }
  }
  async function poll() {
    scan = await api.scanState(SCAN_ID).catch(() => null);
    if (!scan || scan.finished) {
      running = false;
      clearInterval(timer);
    }
  }
  async function stop() {
    await poll();
    clearInterval(timer);
    await api.scanCancel(SCAN_ID);
    running = false;
  }
  onDestroy(() => {
    clearInterval(timer);
    api.scanCancel(SCAN_ID);
  });

  /** host → open ports, in host order */
  const hosts = $derived.by(() => {
    const m = new Map<string, ScanHit[]>();
    for (const h of scan?.hits ?? []) m.set(h.ip, [...(m.get(h.ip) ?? []), h]);
    return [...m.entries()];
  });
  const pct = $derived(scan && scan.total ? Math.floor((scan.done * 100) / scan.total) : 0);

  /** session type to open for a discovered service */
  function action(h: ScanHit): { label: string; icon: string; go: () => void } | null {
    const p = h.port;
    const sess = (kind: string, port: number) => () => {
      const s = app.blank("", kind);
      s.host = h.ip;
      s.port = port;
      app.netTool = null;
      app.editing = s;
    };
    const web = (scheme: string) => () => {
      const s = app.blank("", "browser");
      s.host = `${scheme}://${h.ip}${(scheme === "http" && p === 80) || (scheme === "https" && p === 443) ? "" : ":" + p}/`;
      app.netTool = null;
      app.openSession(s);
    };
    if (p === 22) return { label: "SSH", icon: "ssh", go: sess("ssh", p) };
    if (p === 23) return { label: "Telnet", icon: "telnet", go: sess("telnet", p) };
    if (p === 21) return { label: "FTP", icon: "ftp", go: sess("ftp", p) };
    if (p === 3389) return { label: "RDP", icon: "rdp", go: sess("rdp", p) };
    if (p >= 5900 && p <= 5909) return { label: "VNC", icon: "vnc", go: sess("vnc", p) };
    if ([443, 8443, 6443, 8006].includes(p)) return { label: "Web", icon: "browser", go: web("https") };
    if ([80, 3000, 5000, 8000, 8080, 8888, 9000, 9090].includes(p)) return { label: "Web", icon: "browser", go: web("http") };
    return null;
  }
</script>

<Modal title="Scanner de ports" onclose={() => (app.netTool = null)} width="760px">
  <div class="form">
    <label for="sc-t">Adresses</label>
    <input id="sc-t" bind:value={targets} spellcheck="false" placeholder="192.168.1.0/24, 10.0.0.1-50, serveur.local" disabled={running} />
    <label for="sc-p">Ports</label>
    <span class="row">
      <input id="sc-p" bind:value={ports} spellcheck="false" placeholder="22,80,443,8000-8100 ou « communs »" disabled={running} />
      <select aria-label="Listes de ports" disabled={running} onchange={(e) => { const v = (e.currentTarget as HTMLSelectElement).value; if (v) ports = v; (e.currentTarget as HTMLSelectElement).value = ""; }}>
        <option value="">Listes…</option>
        {#each PRESETS as [l, v] (v)}<option value={v}>{l}</option>{/each}
      </select>
    </span>
    <label for="sc-to">Délai par port</label>
    <span class="row narrow">
      <input id="sc-to" type="number" min="50" max="10000" step="50" bind:value={timeout} disabled={running} /> ms
      <span class="grow"></span>
      {#if running}
        <button onclick={stop}><Icon name="stop" size={16} /> Arrêter</button>
      {:else}
        <button class="primary" onclick={start} data-scan-start><Icon name="scan" size={16} /> Lancer l'analyse</button>
      {/if}
    </span>
  </div>
  {#if error}<p class="error">{error}</p>{/if}

  {#if scan}
    <div class="prog">
      <div class="bar"><div class="fill" style:width="{pct}%"></div></div>
      <span>
        {scan.done} / {scan.total} tests · {scan.hosts} adresse(s) · {hosts.length} machine(s) avec ports ouverts
        {#if scan.finished}— terminé{:else if !running}— interrompu{/if}
      </span>
    </div>
  {/if}

  <div class="results" data-scan-results>
    <table>
      <thead><tr><th>Machine</th><th>Port</th><th>Service</th><th></th></tr></thead>
      <tbody>
        {#each hosts as [ip, hits] (ip)}
          {#each hits as h, i (h.port)}
            {@const a = action(h)}
            <tr class:first={i === 0}>
              <td class="mono">{i === 0 ? (h.host !== ip ? `${h.host} (${ip})` : ip) : ""}</td>
              <td class="mono">{h.port}</td>
              <td>{h.service || "—"}</td>
              <td class="act">
                {#if a}<button class="small" title="Ouvrir une session {a.label} vers {ip}:{h.port}" onclick={a.go}><Icon name={a.icon} size={14} /> {a.label}</button>{/if}
              </td>
            </tr>
          {/each}
        {:else}
          <tr><td colspan="4" class="empty">{scan ? (running ? "Analyse en cours…" : "Aucun port ouvert trouvé.") : "Indique des adresses et des ports, puis lance l'analyse."}</td></tr>
        {/each}
      </tbody>
    </table>
  </div>

  {#snippet actions()}
    <button class="primary dlg" onclick={() => (app.netTool = null)}>Fermer</button>
  {/snippet}
</Modal>

<style>
  .form {
    display: grid;
    grid-template-columns: 110px 1fr;
    align-items: center;
    gap: 6px 8px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .row input {
    flex: 1;
  }
  .narrow input {
    flex: 0 0 90px;
  }
  .grow {
    flex: 1;
  }
  .prog {
    margin-top: 10px;
    display: flex;
    flex-direction: column;
    gap: 3px;
    color: var(--muted);
  }
  .bar {
    height: 8px;
    border: 1px solid var(--chrome-border-dark);
    background: #fff;
  }
  .fill {
    height: 100%;
    background: linear-gradient(#5fbf6f, #2f9a45);
    transition: width 0.2s;
  }
  .results {
    margin-top: 8px;
    height: 300px;
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
    text-align: left;
    font-weight: normal;
    background: var(--chrome);
    border-bottom: 1px solid var(--chrome-border);
    padding: 4px 6px;
  }
  td {
    padding: 2px 6px;
  }
  tr.first td {
    border-top: 1px solid #e6e6e6;
  }
  .mono {
    font-family: var(--mono);
    font-size: 12px;
  }
  .act {
    text-align: right;
  }
  .small {
    padding: 1px 6px;
    font-size: 11.5px;
  }
  .empty {
    color: var(--muted);
    text-align: center;
    padding: 20px;
  }
  .error {
    color: var(--danger);
    margin: 6px 0 0;
  }
  .dlg {
    min-width: 90px;
    justify-content: center;
  }
</style>
