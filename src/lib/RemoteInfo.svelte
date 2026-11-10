<script lang="ts">
  // Information bar about the remote host (CPU, memory, disk, network, uptime…),
  // refreshed every few seconds over SSH while the tab is visible.
  import { onDestroy } from "svelte";
  import { api, asApiError, formatSize, type RemoteStats } from "./api";
  import Icon from "./Icon.svelte";

  let { connId, visible, extra = "" }: { connId: string; visible: boolean; extra?: string } = $props();

  const PERIOD = 3000;
  let cur: RemoteStats | null = $state(null);
  let prev: RemoteStats | null = null;
  let prevAt = 0;
  let cpu = $state<number | null>(null);
  let rx = $state<number | null>(null);
  let tx = $state<number | null>(null);
  let note = $state("Lecture des informations système…");
  let stopped = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let busy = false;

  async function sample() {
    if (busy || stopped) return;
    busy = true;
    try {
      const s = await api.monitorSample(connId);
      const now = Date.now();
      if (!s.supported) {
        note = "Informations système non disponibles pour ce système (Linux requis).";
        stopped = true;
        return;
      }
      if (prev) {
        const dt = s.cpu_total - prev.cpu_total;
        cpu = dt > 0 ? Math.max(0, Math.min(100, (100 * (dt - (s.cpu_idle - prev.cpu_idle))) / dt)) : cpu;
        const sec = (now - prevAt) / 1000;
        if (sec > 0) {
          rx = Math.max(0, (s.net_rx - prev.net_rx) / sec);
          tx = Math.max(0, (s.net_tx - prev.net_tx) / sec);
        }
      }
      prev = s;
      prevAt = now;
      cur = s;
      note = "";
    } catch (e) {
      const err = asApiError(e);
      // permanent conditions (no SSH on the host, unsupported session): stop asking
      if (err.kind === "unsupported" || err.kind === "conn") stopped = true;
      note = err.message.replace(/^Non pris en charge\s*:\s*/i, "");
    } finally {
      busy = false;
    }
  }

  function schedule() {
    clearTimeout(timer);
    if (stopped || !visible) return;
    timer = setTimeout(async () => {
      await sample();
      schedule();
    }, cur ? PERIOD : 50);
  }

  $effect(() => {
    void connId;
    void visible;
    schedule();
  });
  onDestroy(() => {
    stopped = true;
    clearTimeout(timer);
  });

  const pct = (used: number, total: number) => (total > 0 ? Math.round((100 * used) / total) : 0);
  function uptime(sec: number): string {
    const d = Math.floor(sec / 86400);
    const h = Math.floor((sec % 86400) / 3600);
    const m = Math.floor((sec % 3600) / 60);
    return d ? `${d} j ${h} h` : h ? `${h} h ${m} min` : `${m} min`;
  }
  const rate = (v: number | null) => (v === null ? "…" : `${formatSize(Math.round(v))}/s`);
</script>

<div class="info" data-remote-info>
  {#if extra}<span class="it">{extra}</span>{/if}
  {#if cur}
    <span class="it host" title={cur.os}><Icon name="servers" size={13} /> {cur.host}</span>
    <span class="it" title="Processeur ({cur.cpus} cœur(s)) — charge {cur.load}">
      CPU <span class="meter"><span style="width:{cpu ?? 0}%"></span></span> {cpu === null ? "…" : `${Math.round(cpu)} %`}
    </span>
    <span class="it" title="Mémoire utilisée / totale{cur.swap_total ? ` — swap ${formatSize((cur.swap_total - cur.swap_free) * 1024)} / ${formatSize(cur.swap_total * 1024)}` : ''}">
      RAM <span class="meter"><span style="width:{pct(cur.mem_total - cur.mem_avail, cur.mem_total)}%"></span></span>
      {formatSize((cur.mem_total - cur.mem_avail) * 1024)} / {formatSize(cur.mem_total * 1024)}
    </span>
    <span class="it" title="Disque (/)">
      Disque <span class="meter"><span style="width:{pct(cur.disk_used, cur.disk_total)}%"></span></span>
      {pct(cur.disk_used, cur.disk_total)} %
    </span>
    <span class="it" title="Réseau (reçu / envoyé)">↓ {rate(rx)} ↑ {rate(tx)}</span>
    <span class="it" title="Temps depuis le démarrage">Allumé depuis {uptime(cur.uptime)}</span>
    <span class="it" title="Utilisateurs connectés">{cur.users} utilisateur(s)</span>
    <span class="it os">{cur.os}</span>
  {:else}
    <span class="it muted">{note}</span>
  {/if}
  {#if cur && note}<span class="it muted">{note}</span>{/if}
</div>

<style>
  .info {
    display: flex;
    align-items: center;
    gap: 14px;
    height: 22px;
    padding: 0 10px;
    font-size: 11.5px;
    color: var(--text);
    background: var(--chrome);
    border-top: 1px solid var(--chrome-border);
    border-bottom: 1px solid var(--chrome-border);
    white-space: nowrap;
    overflow: hidden;
    user-select: none;
    flex: 0 0 auto;
  }
  .it {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
  .host {
    font-weight: 600;
  }
  .os,
  .muted {
    color: var(--muted);
  }
  .os {
    margin-left: auto;
  }
  .meter {
    display: inline-block;
    width: 46px;
    height: 7px;
    border-radius: 4px;
    background: var(--switch-off);
    overflow: hidden;
  }
  .meter span {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 0.4s;
  }
</style>
