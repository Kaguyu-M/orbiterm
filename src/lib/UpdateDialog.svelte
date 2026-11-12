<script lang="ts">
  // A new version exists. The Windows installer is replaced from here (download, SHA-256
  // check against the publication, then the installer starts and Orbiterm closes); the other
  // kinds of installation are updated by their own tool, whose command is shown.
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api, asApiError } from "./api";
  import { app } from "./app.svelte";
  import { clipWrite } from "./clipboard";
  import Icon from "./Icon.svelte";
  import Modal from "./Modal.svelte";
  import type { UpdateInfo } from "./update";

  let { info, current, onclose, onskip }: { info: UpdateInfo; current: string; onclose: () => void; onskip: () => void } = $props();

  type Mode = Awaited<ReturnType<typeof api.updateMode>>;
  let mode = $state<Mode | null>(null);
  let step = $state<"ask" | "download" | "launch" | "error">("ask");
  let received = $state(0);
  let total = $state<number | null>(null);
  let error = $state("");

  const mb = (n: number) => (n / 1048576).toFixed(1).replace(".", ",");
  const percent = $derived(total ? Math.min(100, Math.round((received / total) * 100)) : null);
  const open = $derived(app.tabs.length);
  const busy = $derived(step === "download" || step === "launch");

  const HOW: Record<string, string> = {
    scoop: "Cette copie a été installée avec Scoop : mettez-la à jour avec la commande",
    deb: "Cette copie vient du paquet .deb : mettez-la à jour avec la commande",
    rpm: "Cette copie vient du paquet .rpm : mettez-la à jour avec la commande",
    nix: "Cette copie vient de Nix : mettez à jour l'entrée du flake, puis reconstruisez votre configuration",
    portable: "Cette copie est la version portable : téléchargez le nouveau zip et remplacez le dossier.",
    other: "Téléchargez la nouvelle version depuis la page de la publication.",
  };

  onMount(() => {
    api
      .updateMode()
      .then((m) => (mode = m))
      .catch(() => (mode = { kind: "other", hint: "" }));
    const un = listen<{ received: number; total: number | null }>("update-progress", (e) => {
      received = e.payload.received;
      total = e.payload.total;
    });
    return () => void un.then((u) => u());
  });

  async function install() {
    step = "download";
    received = 0;
    total = null;
    error = "";
    try {
      await api.updateDownload(info.latest);
      step = "launch";
      await api.updateLaunch(); // Orbiterm closes right after
    } catch (e) {
      error = asApiError(e).message;
      step = "error";
    }
  }
</script>

<Modal title="Mise à jour d'Orbiterm" onclose={() => !busy && onclose()} width="480px">
  <div class="upd" data-update-dialog>
    <p><b>La version {info.latest} est disponible</b> <span class="muted">(vous utilisez la {current}).</span></p>
    {#if !mode}
      <p class="muted">…</p>
    {:else if mode.kind === "installer"}
      {#if step === "ask"}
        <p>Orbiterm télécharge la nouvelle version, vérifie son empreinte SHA-256, l'installe sans rien vous demander, puis redémarre.</p>
        {#if open}<p class="warn"><Icon name="info" size={14} /> {open} onglet{open > 1 ? "s ouverts seront fermés" : " ouvert sera fermé"}.</p>{/if}
      {:else if step === "download"}
        <p data-update-progress>
          Téléchargement… {mb(received)} Mo{total ? ` sur ${mb(total)} Mo` : ""}
        </p>
        <progress max="100" value={percent ?? undefined}></progress>
      {:else if step === "launch"}
        <p>Empreinte vérifiée. Installation en cours, Orbiterm va redémarrer…</p>
      {:else}
        <p class="error" data-update-error>{error}</p>
      {/if}
    {:else}
      <p>{HOW[mode.kind]}{mode.hint ? " :" : ""}</p>
      {#if mode.hint}
        <div class="cmd">
          <code data-update-hint>{mode.hint}</code>
          <button onclick={() => clipWrite(mode!.hint)} title="Copier la commande"><Icon name="copy" size={14} /></button>
        </div>
      {/if}
    {/if}
    <p><button class="link" onclick={() => openUrl(info.url).catch(() => {})}>Voir la publication et les nouveautés</button></p>
  </div>
  {#snippet actions()}
    <button class="dlg" disabled={busy} onclick={onskip} data-update-skip>Ignorer cette version</button>
    <button class="dlg" disabled={busy} onclick={onclose} data-update-later>Plus tard</button>
    {#if mode?.kind === "installer"}
      <button class="primary dlg" disabled={busy} onclick={install} data-update-install>{step === "error" ? "Réessayer" : "Mettre à jour"}</button>
    {/if}
  {/snippet}
</Modal>

<style>
  .upd p {
    margin: 0 0 8px;
    line-height: 1.5;
    white-space: normal;
  }
  .muted {
    color: var(--muted);
  }
  .warn {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .error {
    color: var(--danger);
  }
  progress {
    width: 100%;
    height: 10px;
    margin-bottom: 8px;
    accent-color: var(--accent);
  }
  .cmd {
    display: flex;
    gap: 6px;
    align-items: center;
    margin-bottom: 8px;
  }
  code {
    flex: 1;
    padding: 5px 8px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--input-bg, var(--panel));
    user-select: text;
    overflow-wrap: anywhere;
  }
  button.link {
    background: none;
    border: none;
    padding: 0;
    color: var(--accent);
    cursor: pointer;
    text-decoration: underline;
  }
</style>
