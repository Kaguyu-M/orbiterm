<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import Modal from "./Modal.svelte";
  import type { Folder, Session } from "./api";

  let {
    session,
    folders,
    onsave,
    oncancel,
  }: {
    session: Session;
    folders: Folder[];
    onsave: (s: Session) => void;
    oncancel: () => void;
  } = $props();

  // edit a copy, so "Annuler" really cancels (initial value only is intended)
  // svelte-ignore state_referenced_locally
  let s: Session = $state(structuredClone($state.snapshot(session)) as Session);
  let error = $state("");

  function folderPath(id: string): string {
    const parts: string[] = [];
    let cur = folders.find((f) => f.id === id);
    while (cur) {
      parts.unshift(cur.name);
      cur = folders.find((f) => f.id === cur!.parent);
    }
    return parts.join(" / ");
  }

  async function pickKey() {
    const p = await open({ multiple: false, directory: false, title: "Clé privée SSH" });
    if (typeof p === "string") s.key_path = p;
  }

  function save() {
    s.host = s.host.trim();
    s.username = s.username.trim();
    if (!s.host) return (error = "L'hôte est obligatoire.");
    if (!s.port || s.port < 1 || s.port > 65535) return (error = "Port invalide.");
    if (s.auth === "key" && !s.key_path) return (error = "Choisis un fichier de clé privée.");
    if (!s.name.trim()) s.name = s.username ? `${s.username}@${s.host}` : s.host;
    onsave(s);
  }
</script>

<Modal title={session.id ? "Modifier la session" : "Nouvelle session SSH"} onclose={oncancel} width="520px">
  <form class="grid" onsubmit={(e) => { e.preventDefault(); save(); }}>
    <label for="host">Hôte</label>
    <div class="row">
      <input id="host" bind:value={s.host} placeholder="serveur.exemple.com ou 192.168.1.10" autocomplete="off" />
      <input class="port" type="number" min="1" max="65535" bind:value={s.port} aria-label="Port" />
    </div>

    <label for="user">Utilisateur</label>
    <input id="user" bind:value={s.username} placeholder="root" autocomplete="off" />

    <label for="name">Nom</label>
    <input id="name" bind:value={s.name} placeholder="(par défaut : utilisateur@hôte)" />

    <label for="folder">Dossier</label>
    <select id="folder" bind:value={s.folder}>
      <option value="">(racine)</option>
      {#each folders as f (f.id)}
        <option value={f.id}>{folderPath(f.id)}</option>
      {/each}
    </select>

    <span class="lbl">Authentification</span>
    <div class="row">
      <label class="inline"><input type="radio" bind:group={s.auth} value="password" /> Mot de passe</label>
      <label class="inline"><input type="radio" bind:group={s.auth} value="key" /> Clé privée</label>
    </div>

    {#if s.auth === "password"}
      <span></span>
      <label class="inline">
        <input type="checkbox" bind:checked={s.remember_password} />
        Mémoriser le mot de passe (trousseau du système)
      </label>
    {:else}
      <label for="key">Fichier de clé</label>
      <div class="row">
        <input id="key" bind:value={s.key_path} placeholder="~/.ssh/id_ed25519" />
        <button type="button" onclick={pickKey}>…</button>
      </div>
    {/if}

    <span class="lbl">Navigateur SFTP</span>
    <label class="inline"><input type="checkbox" bind:checked={s.sftp_enabled} /> Ouvrir avec la session</label>

    <label for="fs">Police du terminal</label>
    <div class="row">
      <input id="fs" bind:value={s.terminal.font_family} />
      <input class="port" type="number" min="6" max="40" bind:value={s.terminal.font_size} aria-label="Taille" />
    </div>
    <button type="submit" hidden>ok</button>
  </form>
  {#if error}<p class="error">{error}</p>{/if}

  {#snippet actions()}
    <button onclick={oncancel}>Annuler</button>
    <button class="primary" onclick={save}>Enregistrer</button>
  {/snippet}
</Modal>

<style>
  .grid {
    display: grid;
    grid-template-columns: 130px 1fr;
    gap: 10px 12px;
    align-items: center;
  }
  .row {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .row input:not([type="radio"]) {
    flex: 1;
  }
  .port {
    flex: 0 0 80px !important;
    width: 80px;
  }
  .inline {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .lbl,
  label[for] {
    color: var(--muted);
  }
  .error {
    color: var(--danger);
    margin: 12px 0 0;
  }
</style>
