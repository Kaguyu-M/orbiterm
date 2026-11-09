<script lang="ts">
  import { onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import Modal from "./Modal.svelte";
  import Icon from "./Icon.svelte";
  import { api, asApiError, type ImportReport, type ImportSource } from "./api";
  import { app } from "./app.svelte";

  let sources: ImportSource[] = $state([]);
  let loading = $state(true);
  let busy = $state("");
  let reports: ImportReport[] = $state([]);
  let error = $state("");

  const ICONS: Record<string, string> = { mobaxterm: "session", putty: "ssh", ssh_config: "key" };

  onMount(async () => {
    sources = await api.importSources().catch(() => []);
    loading = false;
  });

  async function run(source: string, path?: string) {
    busy = source;
    error = "";
    try {
      reports = [await app.importFrom(source, path), ...reports];
    } catch (e) {
      error = asApiError(e).message;
    } finally {
      busy = "";
    }
  }

  async function fromFile() {
    const f = await open({
      multiple: false,
      directory: false,
      title: "Fichier de sessions à importer",
      filters: [
        { name: "Sessions MobaXterm (.ini, .moba)", extensions: ["ini", "moba", "mxtsessions"] },
        { name: "Tous les fichiers (config OpenSSH…)", extensions: ["*"] },
      ],
    });
    if (typeof f === "string") await run("file", f);
  }
</script>

<Modal title="Importer des sessions" onclose={() => (app.showImport = false)} width="640px">
  <p class="intro">
    Les sessions sont ajoutées dans un dossier « Import … » en conservant l'arborescence d'origine. Les mots de passe ne sont
    <b>jamais</b> importés : ils seront demandés à la première connexion. Une session déjà importée n'est pas dupliquée.
  </p>

  <div class="sources">
    {#if loading}
      <p class="muted">Recherche des sessions sur ce poste…</p>
    {:else}
      {#each sources as s (s.id)}
        <div class="src" data-source={s.id}>
          <Icon name={ICONS[s.id] ?? "file"} size={28} />
          <div class="info">
            <b>{s.label}</b> — {s.count} session(s)
            <div class="path" title={s.path}>{s.path}</div>
          </div>
          <button class="primary" disabled={!!busy || !s.count} onclick={() => run(s.id)}>
            {busy === s.id ? "Import…" : "Importer"}
          </button>
        </div>
      {:else}
        <p class="muted">Aucune configuration MobaXterm, PuTTY ou OpenSSH trouvée sur ce poste.</p>
      {/each}
    {/if}
    <div class="src">
      <Icon name="folder_open" size={28} />
      <div class="info">
        <b>Depuis un fichier…</b>
        <div class="path">MobaXterm.ini, export .moba / .mxtsessions, ou fichier de configuration OpenSSH</div>
      </div>
      <button disabled={!!busy} onclick={fromFile}>{busy === "file" ? "Import…" : "Choisir…"}</button>
    </div>
  </div>

  {#if error}<p class="error">{error}</p>{/if}

  {#each reports as r, i (i)}
    <div class="report" data-report>
      <Icon name="info" size={18} />
      <div>
        <b>{r.source}</b> : {r.added} session(s) ajoutée(s), {r.folders} dossier(s) créé(s){r.duplicates ? `, ${r.duplicates} déjà présente(s)` : ""}.
        {#if r.skipped.length}
          <details>
            <summary>{r.skipped.length} entrée(s) ignorée(s)</summary>
            <ul>{#each r.skipped as sk (sk)}<li>{sk}</li>{/each}</ul>
          </details>
        {/if}
      </div>
    </div>
  {/each}

  {#snippet actions()}
    <button class="primary dlg" onclick={() => (app.showImport = false)}>Fermer</button>
  {/snippet}
</Modal>

<style>
  .intro {
    margin: 0 0 10px;
  }
  .sources {
    border: 1px solid var(--chrome-border);
  }
  .src {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border-bottom: 1px solid var(--chrome-border);
  }
  .src:last-child {
    border-bottom: none;
  }
  .info {
    flex: 1;
    min-width: 0;
  }
  .path {
    color: var(--muted);
    font-size: 11.5px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .src button {
    min-width: 90px;
    justify-content: center;
  }
  .report {
    display: flex;
    gap: 8px;
    margin-top: 10px;
    padding: 8px;
    background: #eefaf1;
    border: 1px solid #b7e2c3;
  }
  .report ul {
    margin: 4px 0 0;
    padding-left: 18px;
    color: var(--muted);
  }
  .error {
    color: var(--danger);
  }
  .muted {
    padding: 8px 10px;
    margin: 0;
  }
  .dlg {
    min-width: 90px;
    justify-content: center;
  }
</style>
