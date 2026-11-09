<script lang="ts">
  import { onMount } from "svelte";
  import Modal from "./Modal.svelte";
  import Icon from "./Icon.svelte";
  import { api } from "./api";
  import { app, DEFAULT_PREFS, type Prefs } from "./app.svelte";

  type TabId = "general" | "terminal" | "ssh" | "x11" | "display";
  const TABS: Array<{ id: TabId; label: string; icon: string }> = [
    { id: "general", label: "Général", icon: "settings" },
    { id: "terminal", label: "Terminal", icon: "terminal" },
    { id: "ssh", label: "SSH", icon: "ssh" },
    { id: "x11", label: "Serveur X", icon: "xserver" },
    { id: "display", label: "Affichage", icon: "view" },
  ];
  let tab: TabId = $state("general");
  let p: Prefs = $state(structuredClone($state.snapshot(app.prefs)) as Prefs);
  let cfgDir = $state("");

  const FONTS = [
    "Cascadia Mono, Consolas, 'DejaVu Sans Mono', monospace",
    "Consolas, monospace",
    "'Courier New', monospace",
    "'Lucida Console', monospace",
    "'DejaVu Sans Mono', monospace",
  ];
  const fontLabel = (f: string) => f.split(",")[0].replaceAll("'", "");

  onMount(async () => {
    cfgDir = await api.configPath().catch(() => "");
  });

  function save() {
    Object.assign(app.prefs, $state.snapshot(p));
    app.savePrefs();
    app.showSettings = false;
  }

  function reset() {
    const keep = { sidebarWidth: p.sidebarWidth };
    p = { ...structuredClone(DEFAULT_PREFS), ...keep };
  }

  /** Applies the default terminal settings to every saved session. */
  async function applyToAll() {
    if (!(await app.confirm(`Appliquer la police, la taille, le thème et l'historique à toutes les sessions enregistrées (${app.store.sessions.length}) ?`))) return;
    for (const s of app.store.sessions) {
      const copy = structuredClone($state.snapshot(s));
      copy.terminal = { font_family: p.termFont, font_size: p.termFontSize, theme: p.termTheme, scrollback: p.termScrollback };
      await api.sessionSave(copy);
    }
    await app.reload();
    app.info("Réglages du terminal appliqués à toutes les sessions (effet à la prochaine ouverture).");
  }
</script>

<Modal title="Configuration d'Orbiterm" onclose={() => (app.showSettings = false)} width="680px">
  <div class="tabs" role="tablist">
    {#each TABS as t (t.id)}
      <button role="tab" class:on={tab === t.id} aria-selected={tab === t.id} data-settab={t.id} onclick={() => (tab = t.id)}>
        <Icon name={t.icon} size={22} />
        <span>{t.label}</span>
      </button>
    {/each}
  </div>

  <div class="panel">
    {#if tab === "general"}
      <fieldset>
        <legend>Démarrage et fermeture</legend>
        <label class="chk"><input type="checkbox" bind:checked={p.confirmQuit} /> Demander confirmation avant de quitter si des sessions sont ouvertes</label>
        <div class="row">
          <label for="defsh">Terminal local par défaut</label>
          <select id="defsh" bind:value={p.defaultShell}>
            <option value="">(premier détecté{app.shells[0] ? ` : ${app.shells[0].label}` : ""})</option>
            {#each app.shells as sh (sh.id)}<option value={sh.id}>{sh.label}</option>{/each}
          </select>
        </div>
      </fieldset>
      <fieldset>
        <legend>Données</legend>
        <div class="row">
          <label for="cfg">Dossier de configuration</label>
          <input id="cfg" readonly value={cfgDir} />
          <button onclick={() => navigator.clipboard.writeText(cfgDir)} title="Copier le chemin"><Icon name="copy" size={14} /></button>
        </div>
        <p class="muted">Sessions, dossiers, tunnels et macros y sont enregistrés (sessions.toml). Les mots de passe restent dans le trousseau du système.</p>
        <button onclick={() => { app.showSettings = false; app.showImport = true; }}><Icon name="download" size={14} /> Importer des sessions (MobaXterm, PuTTY, OpenSSH)…</button>
      </fieldset>
    {:else if tab === "terminal"}
      <fieldset>
        <legend>Apparence par défaut des nouvelles sessions</legend>
        <div class="grid">
          <label for="tf">Police</label>
          <select id="tf" bind:value={p.termFont}>
            {#each FONTS as f (f)}<option value={f}>{fontLabel(f)}</option>{/each}
          </select>
          <label for="ts">Taille</label>
          <input id="ts" type="number" min="6" max="40" bind:value={p.termFontSize} />
          <label for="tt">Thème</label>
          <select id="tt" bind:value={p.termTheme}>
            <option value="dark">Sombre (noir)</option>
            <option value="light">Clair</option>
            <option value="solarized">Solarized</option>
          </select>
          <label for="tsb">Historique (lignes)</label>
          <input id="tsb" type="number" min="100" max="200000" step="1000" bind:value={p.termScrollback} />
        </div>
        <button onclick={applyToAll} disabled={!app.store.sessions.length}>Appliquer à toutes les sessions enregistrées</button>
      </fieldset>
      <fieldset>
        <legend>Comportement de tous les terminaux</legend>
        <div class="row">
          <label for="cs">Curseur</label>
          <select id="cs" bind:value={p.cursorStyle}>
            <option value="block">Bloc</option>
            <option value="underline">Souligné</option>
            <option value="bar">Barre verticale</option>
          </select>
          <label class="chk"><input type="checkbox" bind:checked={p.cursorBlink} /> Clignotant</label>
        </div>
        <label class="chk"><input type="checkbox" bind:checked={p.copyOnSelect} /> Copier automatiquement le texte sélectionné</label>
        <label class="chk"><input type="checkbox" bind:checked={p.rightClickPaste} /> Coller avec le clic droit</label>
        <label class="chk"><input type="checkbox" bind:checked={p.visualBell} /> Sonnerie visuelle (le terminal clignote)</label>
      </fieldset>
    {:else if tab === "ssh"}
      <fieldset>
        <legend>Connexions SSH</legend>
        <div class="row">
          <label for="ka">Maintien de la connexion (keepalive) toutes les</label>
          <input id="ka" class="num" type="number" min="0" max="3600" bind:value={p.keepalive} />
          <span>secondes (0 = désactivé)</span>
        </div>
      </fieldset>
      <fieldset>
        <legend>Valeurs par défaut des nouvelles sessions SSH</legend>
        <label class="chk"><input type="checkbox" bind:checked={p.sshSftp} /> Ouvrir le navigateur SFTP avec la session</label>
        <label class="chk"><input type="checkbox" bind:checked={p.sshX11} /> Redirection X11</label>
        <label class="chk"><input type="checkbox" bind:checked={p.sshCompression} /> Compression</label>
      </fieldset>
    {:else if tab === "x11"}
      <fieldset>
        <legend>Serveur X</legend>
        <label class="chk"><input type="checkbox" bind:checked={p.xAutoStart} /> Démarrer automatiquement le serveur X à l'ouverture d'une session avec X11</label>
        <p class="status">
          <Icon name={app.xserver?.running ? "xserver" : "xserver_off"} size={18} />
          {#if !app.xserver?.kind}
            Aucun serveur X détecté. Installe <b>VcXsrv</b> (recommandé) ou Xming ; Orbiterm le lancera en mode multi-fenêtres.
          {:else if app.xserver.kind === "native"}
            Serveur X du système : {app.xserver.display}
          {:else}
            {app.xserver.kind === "vcxsrv" ? "VcXsrv" : "Xming"} — {app.xserver.path}<br />
            État : {app.xserver.running ? `actif (${app.xserver.display})` : "arrêté"}
          {/if}
        </p>
      </fieldset>
    {:else}
      <fieldset>
        <legend>Éléments de la fenêtre</legend>
        <label class="chk"><input type="checkbox" bind:checked={p.menubar} /> Barre de menus</label>
        <label class="chk"><input type="checkbox" bind:checked={p.toolbar} /> Barre d'outils</label>
        <label class="chk"><input type="checkbox" bind:checked={p.sidebar} /> Barre latérale (sessions, outils, macros, SFTP)</label>
        <label class="chk"><input type="checkbox" bind:checked={p.statusbar} /> Barre d'état</label>
        <div class="row">
          <label for="sw">Largeur de la barre latérale</label>
          <input id="sw" class="num" type="number" min="180" max="600" bind:value={p.sidebarWidth} /> px
        </div>
      </fieldset>
    {/if}
  </div>

  {#snippet actions()}
    <button class="reset" onclick={reset}>Valeurs par défaut</button>
    <span class="grow"></span>
    <button class="primary dlg" onclick={save}>OK</button>
    <button class="dlg" onclick={() => (app.showSettings = false)}>Annuler</button>
  {/snippet}
</Modal>

<style>
  .tabs {
    display: flex;
    gap: 2px;
    border-bottom: 1px solid var(--chrome-border-dark);
    margin: -4px 0 0;
  }
  .tabs button {
    flex-direction: column;
    gap: 2px;
    min-width: 76px;
    padding: 5px 8px 4px;
    border: 1px solid transparent;
    border-bottom: none;
    border-radius: 3px 3px 0 0;
    background: none;
    font-size: 12px;
  }
  .tabs button.on {
    background: var(--panel);
    border-color: var(--chrome-border-dark);
    margin-bottom: -1px;
    box-shadow: inset 0 2px 0 var(--accent);
  }
  .panel {
    min-height: 330px;
    padding-top: 10px;
  }
  fieldset {
    border: 1px solid var(--chrome-border);
    margin: 0 0 10px;
    padding: 6px 12px 10px;
    display: flex;
    flex-direction: column;
    gap: 7px;
  }
  legend {
    color: var(--brand);
    font-weight: 600;
    padding: 0 4px;
  }
  .chk {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .row input:not(.num):not([type="checkbox"]),
  .row select {
    flex: 1;
  }
  .grid {
    display: grid;
    grid-template-columns: auto 1fr auto 1fr;
    gap: 6px 10px;
    align-items: center;
  }
  .num {
    width: 70px;
  }
  fieldset > button {
    align-self: flex-start;
  }
  .status {
    display: flex;
    gap: 8px;
    align-items: flex-start;
    background: var(--chrome);
    border: 1px solid var(--chrome-border);
    padding: 8px;
    margin: 0;
  }
  .muted {
    margin: 0;
  }
  .grow {
    flex: 1;
  }
  .dlg {
    min-width: 90px;
    justify-content: center;
  }
</style>
