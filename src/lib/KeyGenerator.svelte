<script lang="ts">
  import { onDestroy } from "svelte";
  import Modal from "./Modal.svelte";
  import Icon from "./Icon.svelte";
  import { app } from "./app.svelte";
  import { api, asApiError, type KeyInfo } from "./api";
  import { clipWrite } from "./clipboard";

  const TYPES: Array<[string, string]> = [
    ["ed25519", "Ed25519 (recommandé)"],
    ["ecdsa256", "ECDSA P-256"],
    ["ecdsa384", "ECDSA P-384"],
    ["ecdsa521", "ECDSA P-521"],
    ["rsa2048", "RSA 2048 bits"],
    ["rsa3072", "RSA 3072 bits"],
    ["rsa4096", "RSA 4096 bits"],
  ];
  let kind = $state("ed25519");
  let comment = $state(defaultComment());
  let pass1 = $state("");
  let pass2 = $state("");
  let key: KeyInfo | null = $state(null);
  let busy = $state(false);
  let msg = $state("");
  let error = $state("");
  const mismatch = $derived(pass1 !== pass2);

  /** Automated GUI tests replace the native file dialogs: window.__orbitermKeyPick = { open, save }. */
  type Pick = { open?: string; save?: (defaultPath: string) => string };
  const testPick = () => (window as unknown as { __orbitermKeyPick?: Pick }).__orbitermKeyPick;
  async function pickOpen(title: string): Promise<string | null> {
    const t = testPick();
    if (t?.open) return t.open;
    const { open } = await import("@tauri-apps/plugin-dialog");
    const p = await open({ multiple: false, directory: false, title });
    return typeof p === "string" ? p : null;
  }
  async function pickSave(opts: { title: string; defaultPath: string; filters?: Array<{ name: string; extensions: string[] }> }): Promise<string | null> {
    const t = testPick();
    if (t?.save) return t.save(opts.defaultPath);
    const { save } = await import("@tauri-apps/plugin-dialog");
    return save(opts);
  }

  function defaultComment() {
    const d = new Date();
    return `orbiterm-${d.getFullYear()}${String(d.getMonth() + 1).padStart(2, "0")}${String(d.getDate()).padStart(2, "0")}`;
  }

  async function run(f: () => Promise<void>) {
    msg = "";
    error = "";
    busy = true;
    try {
      await f();
    } catch (e) {
      error = asApiError(e).message;
    } finally {
      busy = false;
    }
  }

  const generate = () =>
    run(async () => {
      key = await api.keyGenerate(kind, comment.trim());
      comment = key.comment;
      msg = "Clé générée. Enregistre maintenant la clé privée (garde-la pour toi) et la clé publique (à installer sur les serveurs).";
    });

  const load = () =>
    run(async () => {
      const path = await pickOpen("Charger une clé privée (OpenSSH ou PuTTY .ppk)");
      if (!path) return;
      let pass: string | undefined;
      for (;;) {
        try {
          key = await api.keyLoad(path, pass);
          break;
        } catch (e) {
          const err = asApiError(e);
          if (err.kind !== "need_passphrase" && !/phrase secrète incorrecte/.test(err.message)) throw e;
          const p = await app.ask(`${pass === undefined ? "" : "⚠ Phrase secrète incorrecte — "}Phrase secrète de ${path.split(/[\\/]/).pop()}`, "", true);
          if (p === null) return;
          pass = p.value;
        }
      }
      comment = key.comment;
      msg = "Clé chargée : tu peux la réenregistrer dans l'autre format ou changer sa phrase secrète.";
    });

  async function setComment() {
    if (key && comment.trim() !== key.comment) key = await api.keySetComment(comment.trim());
  }

  const savePrivate = (format: "openssh" | "ppk") =>
    run(async () => {
      await setComment();
      const base = key!.algorithm === "ssh-ed25519" ? "id_ed25519" : key!.algorithm.startsWith("ecdsa") ? "id_ecdsa" : "id_rsa";
      const path = await pickSave({
        title: format === "ppk" ? "Enregistrer la clé privée (PuTTY .ppk)" : "Enregistrer la clé privée (OpenSSH)",
        defaultPath: format === "ppk" ? `${base}.ppk` : base,
        filters: format === "ppk" ? [{ name: "Clé PuTTY", extensions: ["ppk"] }] : undefined,
      });
      if (!path) return;
      await api.keySavePrivate(path, format, pass1);
      msg = `Clé privée enregistrée${pass1 ? " (protégée par la phrase secrète)" : " sans phrase secrète"} : ${path}`;
    });

  const savePublic = () =>
    run(async () => {
      await setComment();
      const base = key!.algorithm === "ssh-ed25519" ? "id_ed25519" : key!.algorithm.startsWith("ecdsa") ? "id_ecdsa" : "id_rsa";
      const path = await pickSave({ title: "Enregistrer la clé publique", defaultPath: `${base}.pub`, filters: [{ name: "Clé publique", extensions: ["pub"] }] });
      if (!path) return;
      await api.keySavePublic(path);
      msg = `Clé publique enregistrée : ${path}`;
    });

  const copyPublic = () =>
    run(async () => {
      await setComment();
      await clipWrite(key!.public_openssh);
      msg = "Clé publique copiée : colle-la dans ~/.ssh/authorized_keys sur le serveur.";
    });

  function close() {
    app.netTool = null;
  }
  // the private key never outlives the window
  onDestroy(() => void api.keyForget().catch(() => {}));
</script>

<Modal title="Générateur de clés SSH" onclose={close} width="680px">
  <div class="form">
    <label for="kg-t">Type de clé</label>
    <span class="row">
      <select id="kg-t" bind:value={kind} disabled={busy}>
        {#each TYPES as [v, l] (v)}<option value={v}>{l}</option>{/each}
      </select>
      <button class="primary" onclick={generate} disabled={busy} data-kg-generate><Icon name="key" size={14} /> {busy && !key ? "Génération…" : "Générer"}</button>
      <button onclick={load} disabled={busy} data-kg-load><Icon name="folder" size={14} /> Charger une clé…</button>
    </span>
  </div>

  {#if key}
    <div class="key">
      <div class="pub">
        <div class="lt">Clé publique (à ajouter dans <code>~/.ssh/authorized_keys</code>)</div>
        <textarea readonly rows="4" spellcheck="false" data-kg-public onfocus={(e) => e.currentTarget.select()}>{key.public_openssh}</textarea>
        <div class="meta">
          <span>{key.algorithm} · {key.bits} bits</span>
          <span class="mono" data-kg-fp>{key.fingerprint}</span>
        </div>
      </div>
      <pre class="art" title="Représentation visuelle de l'empreinte">{key.randomart}</pre>
    </div>
    <div class="form">
      <label for="kg-c">Commentaire</label>
      <input id="kg-c" bind:value={comment} onchange={() => run(setComment)} spellcheck="false" />
      <label for="kg-p1">Phrase secrète</label>
      <input id="kg-p1" type="password" bind:value={pass1} placeholder="(aucune — déconseillé)" autocomplete="new-password" />
      <label for="kg-p2">Confirmation</label>
      <input id="kg-p2" type="password" bind:value={pass2} class:bad={mismatch} autocomplete="new-password" />
    </div>
    <div class="save">
      <button onclick={copyPublic} disabled={busy}><Icon name="copy" size={14} /> Copier la clé publique</button>
      <button onclick={savePublic} disabled={busy}>Enregistrer la clé publique…</button>
      <button onclick={() => savePrivate("openssh")} disabled={busy || mismatch} data-kg-save-openssh>Enregistrer la clé privée (OpenSSH)…</button>
      <button onclick={() => savePrivate("ppk")} disabled={busy || mismatch} data-kg-save-ppk>Enregistrer au format PuTTY (.ppk)…</button>
    </div>
  {:else}
    <p class="muted">Crée une paire de clés pour te connecter sans mot de passe, ou charge une clé existante pour la convertir entre les formats OpenSSH et PuTTY (.ppk) ou changer sa phrase secrète.</p>
  {/if}
  {#if mismatch && key}<p class="error">Les deux phrases secrètes sont différentes.</p>{/if}
  {#if msg}<p class="ok" data-kg-msg>{msg}</p>{/if}
  {#if error}<p class="error" data-kg-error>{error}</p>{/if}

  {#snippet actions()}
    <button class="dlg" onclick={close}>Fermer</button>
  {/snippet}
</Modal>

<style>
  .form {
    display: grid;
    grid-template-columns: 120px 1fr;
    align-items: center;
    gap: 6px 8px;
    margin-bottom: 8px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .row select {
    flex: 1;
  }
  .key {
    display: flex;
    gap: 10px;
    margin: 6px 0 10px;
  }
  .pub {
    flex: 1;
    min-width: 0;
  }
  textarea {
    width: 100%;
    box-sizing: border-box;
    font-family: var(--mono, monospace);
    font-size: 11px;
    resize: none;
    word-break: break-all;
  }
  .meta {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    font-size: 11px;
    color: var(--muted);
  }
  .mono {
    font-family: var(--mono, monospace);
  }
  .art {
    margin: 18px 0 0;
    font-size: 10px;
    line-height: 1.15;
    padding: 4px 6px;
    border: 1px solid var(--chrome-border);
    background: var(--panel, transparent);
  }
  .lt {
    font-weight: 600;
    margin-bottom: 3px;
  }
  .save {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .bad {
    border-color: var(--danger);
  }
  .muted {
    color: var(--muted);
  }
  .ok {
    color: var(--ok);
    word-break: break-all;
  }
  .error {
    color: var(--danger);
  }
</style>
