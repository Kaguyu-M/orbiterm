<script lang="ts">
  // Saved passwords manager: lists what Orbiterm keeps in the system keyring. The values
  // never reach the interface: a password can only be replaced or removed here.
  import { onMount } from "svelte";
  import Modal from "./Modal.svelte";
  import Icon from "./Icon.svelte";
  import { api, asApiError, type SavedSecret } from "./api";
  import { app } from "./app.svelte";

  const ROLES: Record<SavedSecret["role"], string> = {
    session: "Session",
    jump: "Passerelle SSH",
    vnc: "Mot de passe VNC",
    gateway: "Passerelle RD",
    server: "Serveur intégré",
  };

  let rows = $state<SavedSecret[] | null>(null);
  let error = $state("");
  let filter = $state("");
  let picked: string[] = $state([]);

  const shown = $derived(
    (rows ?? []).filter((r) => {
      const f = filter.trim().toLowerCase();
      return !f || `${r.name} ${r.user} ${r.host} ${r.kind} ${ROLES[r.role]}`.toLowerCase().includes(f);
    }),
  );

  async function refresh() {
    error = "";
    try {
      rows = await api.passwordsList();
      picked = picked.filter((a) => rows!.some((r) => r.account === a));
    } catch (e) {
      rows = [];
      error = asApiError(e).message;
    }
  }
  onMount(refresh);

  const label = (r: SavedSecret) => `${ROLES[r.role].toLowerCase()} « ${r.name} »`;

  async function replace(r: SavedSecret) {
    const a = await app.ask(`Nouveau mot de passe — ${label(r)}`, "", true);
    if (!a?.value) return;
    try {
      await api.passwordReplace(r.account, a.value);
      app.info("Mot de passe remplacé dans le trousseau.");
    } catch (e) {
      error = asApiError(e).message;
    }
  }

  async function remove(accounts: string[], what: string) {
    if (!accounts.length || !(await app.confirm(`Supprimer ${what} du trousseau ? Il sera redemandé à la prochaine connexion.`))) return;
    try {
      await api.passwordsRemove(accounts);
    } catch (e) {
      error = asApiError(e).message;
    }
    await refresh();
  }

  function toggle(account: string, on: boolean) {
    picked = on ? [...picked.filter((a) => a !== account), account] : picked.filter((a) => a !== account);
  }
  const allPicked = $derived(shown.length > 0 && shown.every((r) => picked.includes(r.account)));
  function toggleAll(on: boolean) {
    picked = on ? shown.map((r) => r.account) : [];
  }
</script>

<Modal title="Gestion des mots de passe" onclose={() => (app.showPasswords = false)} width="720px" align="end">
  <div data-passwords>
    <p class="muted">
      Les mots de passe enregistrés sont conservés dans le trousseau du système (Gestionnaire d'identification de Windows, GNOME Keyring, KWallet…), jamais dans les fichiers d'Orbiterm. Ils ne sont pas affichés ici : tu peux
      les remplacer ou les supprimer.
    </p>
    <div class="bar">
      <input placeholder="Filtrer…" bind:value={filter} aria-label="Filtrer" spellcheck="false" />
      <button type="button" onclick={refresh} title="Rafraîchir" aria-label="Rafraîchir"><Icon name="refresh" size={14} /></button>
    </div>
    {#if error}<p class="err">{error}</p>{/if}
    {#if rows === null}
      <p class="muted">Lecture du trousseau…</p>
    {:else if !rows.length}
      <p class="muted" data-pw-empty>Aucun mot de passe enregistré.</p>
    {:else}
      <div class="scroll">
        <table>
          <thead>
            <tr>
              <th class="c"><input type="checkbox" aria-label="Tout sélectionner" checked={allPicked} onchange={(e) => toggleAll(e.currentTarget.checked)} /></th>
              <th>Nom</th>
              <th>Type</th>
              <th>Compte</th>
              <th>Serveur</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {#each shown as r (r.account)}
              <tr data-pw-row={r.account}>
                <td class="c"><input type="checkbox" aria-label="Sélectionner {r.name}" checked={picked.includes(r.account)} onchange={(e) => toggle(r.account, e.currentTarget.checked)} /></td>
                <td class="name" title={r.name}>{r.name}</td>
                <td>{ROLES[r.role]}{r.kind && r.role === "session" ? ` ${r.kind.toUpperCase()}` : ""}</td>
                <td>{r.user || "—"}</td>
                <td class="host" title={r.host}>{r.host}</td>
                <td class="act">
                  <button type="button" class="small" title="Remplacer le mot de passe" aria-label="Remplacer" onclick={() => replace(r)} data-pw-replace><Icon name="edit" size={14} /></button>
                  <button type="button" class="small" title="Supprimer du trousseau" aria-label="Supprimer" onclick={() => remove([r.account], `le mot de passe de la ${label(r)}`)} data-pw-remove
                    ><Icon name="trash" size={14} /></button
                  >
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
      <p class="muted">{rows.length} mot(s) de passe enregistré(s){shown.length !== rows.length ? ` — ${shown.length} affiché(s)` : ""}.</p>
    {/if}
  </div>
  {#snippet actions()}
    <button class="dlg" disabled={!picked.length} onclick={() => remove(picked, `${picked.length} mot(s) de passe`)} data-pw-remove-picked>Supprimer la sélection</button>
    <button class="primary dlg" onclick={() => (app.showPasswords = false)}>Fermer</button>
  {/snippet}
</Modal>

<style>
  .muted {
    color: var(--muted);
    font-size: 12px;
    margin: 4px 0 8px;
  }
  .err {
    color: var(--danger, #e06c5a);
    font-size: 12px;
  }
  .bar {
    display: flex;
    gap: 6px;
    margin-bottom: 6px;
  }
  .bar input {
    flex: 1;
  }
  .scroll {
    max-height: 340px;
    overflow: auto;
    border: 1px solid var(--chrome-border);
    border-radius: 4px;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 12px;
  }
  th {
    position: sticky;
    top: 0;
    background: var(--chrome, #2b2622);
    text-align: left;
    font-weight: 600;
    padding: 4px 6px;
    border-bottom: 1px solid var(--chrome-border);
  }
  td {
    padding: 3px 6px;
    border-bottom: 1px solid var(--row-border);
    vertical-align: middle;
  }
  .c {
    text-align: center;
    width: 26px;
  }
  .name,
  .host {
    max-width: 180px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .name {
    font-weight: 600;
  }
  .act {
    white-space: nowrap;
    text-align: right;
  }
</style>
