<script lang="ts">
  import { clipWrite } from "./clipboard";
  import { onMount } from "svelte";
  import Modal from "./Modal.svelte";
  import Icon from "./Icon.svelte";
  import { api, type AgentState } from "./api";
  import { open } from "@tauri-apps/plugin-dialog";
  import { app, DEFAULT_PREFS, UI_THEMES, type Prefs } from "./app.svelte";
  import { ANSI_KEYS, BASE_KEYS, fullColors, parseScheme, themeChoices, withAlpha, type CustomTheme } from "./termthemes";
  import { familyOf, firstName, fontChoices } from "./fonts";
  import { imageUrl } from "./termbg";

  type TabId = "general" | "terminal" | "style" | "ssh" | "x11" | "display";
  const TABS: Array<{ id: TabId; label: string; icon: string }> = [
    { id: "general", label: "Général", icon: "settings" },
    { id: "terminal", label: "Terminal", icon: "terminal" },
    { id: "style", label: "Style", icon: "palette" },
    { id: "ssh", label: "SSH", icon: "ssh" },
    { id: "x11", label: "Serveur X", icon: "xserver" },
    { id: "display", label: "Affichage", icon: "view" },
  ];
  let tab: TabId = $state("general");
  let p: Prefs = $state(structuredClone($state.snapshot(app.prefs)) as Prefs);
  let cfgDir = $state("");

  // ---- style of the terminals: fonts, spacing, colours, background
  const FONTS = fontChoices();
  /** the font list shows the current font even when it is not one of the proposed ones */
  const fontKnown = $derived(FONTS.some((f) => f.family === p.termFont));
  let fontOther = $state(false);
  function pickFont(v: string) {
    if (v === "__other") {
      fontOther = true;
      return;
    }
    fontOther = false;
    p.termFont = v;
  }
  const WEIGHTS: Array<[string, string]> = [
    ["300", "Fine (300)"],
    ["normal", "Normale (400)"],
    ["500", "Moyenne (500)"],
    ["600", "Demi-gras (600)"],
    ["bold", "Gras (700)"],
    ["800", "Très gras (800)"],
  ];

  /** custom theme open in the editor (its id) */
  let editingTheme = $state("");
  const edited = $derived(p.customThemes.find((t) => t.id === editingTheme));
  function newThemeId() {
    return "custom:" + Math.random().toString(36).slice(2, 10);
  }
  /** New custom theme, starting from the colours of `base`. */
  function addTheme(base: string, name: string) {
    const t: CustomTheme = { id: newThemeId(), name, colors: fullColors(base, p.customThemes) };
    p.customThemes = [...p.customThemes, t];
    editingTheme = t.id;
    p.termTheme = t.id;
  }
  function removeTheme(id: string) {
    p.customThemes = p.customThemes.filter((t) => t.id !== id);
    if (p.termTheme === id) p.termTheme = "orbiterm";
    if (editingTheme === id) editingTheme = "";
  }
  let importText = $state("");
  let importOpen = $state(false);
  function importTheme() {
    const s = parseScheme(importText);
    if (!s) {
      app.alert("Thème non reconnu : colle un schéma de couleurs JSON (Windows Terminal, ou objet de thème xterm.js).");
      return;
    }
    const t: CustomTheme = { id: newThemeId(), name: s.name, colors: { ...fullColors("dark"), ...s.colors } };
    p.customThemes = [...p.customThemes, t];
    p.termTheme = t.id;
    editingTheme = t.id;
    importOpen = false;
    importText = "";
  }

  async function pickBackground() {
    // automated GUI tests replace the native dialog (window.__orbitermKeyPick.open)
    const pick = (window as unknown as { __orbitermKeyPick?: { open?: string } }).__orbitermKeyPick?.open;
    const f =
      pick ??
      (await open({
        multiple: false,
        directory: false,
        title: "Image de fond des terminaux",
        filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"] }],
      }));
    if (typeof f !== "string") return;
    try {
      await imageUrl(f);
      p.termBgImage = f;
    } catch (e) {
      app.alert((e as { message?: string })?.message ?? String(e));
    }
  }

  // preview of the style being set
  const preview = $derived(fullColors(p.termTheme, p.customThemes));
  let previewBg = $state("");
  $effect(() => {
    const path = p.termBgImage;
    if (!path) {
      previewBg = "";
      return;
    }
    imageUrl(path).then((u) => (previewBg = u)).catch(() => (previewBg = ""));
  });
  const FITS: Record<string, string> = { cover: "center / cover no-repeat", contain: "center / contain no-repeat", center: "center no-repeat", repeat: "repeat" };
  const previewStyle = $derived.by(() => {
    const cell = previewBg ? withAlpha(preview.background, p.termBgOpacity / 100) : preview.background;
    const back = previewBg ? `linear-gradient(${cell}, ${cell}), url("${previewBg}") ${FITS[p.termBgFit] ?? FITS.cover}, ${preview.background}` : cell;
    const weight = p.termFontWeight === "normal" ? "400" : p.termFontWeight;
    return `background: ${back}; color: ${preview.foreground}; font-family: ${p.termFont}; font-size: ${p.termFontSize}px; line-height: ${p.termLineHeight}; letter-spacing: ${p.termLetterSpacing}px; font-weight: ${weight};`;
  });
  const boldWeight = $derived(p.termFontWeightBold === "bold" ? "700" : p.termFontWeightBold);

  onMount(async () => {
    cfgDir = await api.configPath().catch(() => "");
  });

  // ---- SSH agent (keys are loaded / removed immediately; "at start-up" is saved with the settings)
  let agent: AgentState | null = $state(null);
  async function refreshAgent() {
    agent = await api.agentState().catch(() => null);
  }
  $effect(() => {
    if (tab === "ssh" && !agent) refreshAgent();
  });
  async function addAgentKey() {
    // automated GUI tests replace the native dialog (window.__orbitermKeyPick.open)
    const pick = (window as unknown as { __orbitermKeyPick?: { open?: string } }).__orbitermKeyPick?.open;
    const f = pick ?? (await open({ multiple: false, directory: false, title: "Clé privée à charger dans l'agent (OpenSSH ou PuTTY .ppk)" }));
    if (typeof f !== "string") return;
    if (await app.loadAgentKey(f)) {
      if (!p.agentKeys.includes(f)) p.agentKeys = [...p.agentKeys, f];
      await refreshAgent();
    }
  }
  async function removeAgentKey(fp: string, source: string) {
    await api.agentRemove(fp).catch(() => {});
    p.agentKeys = p.agentKeys.filter((k) => k !== source);
    await refreshAgent();
  }
  function toggleStartup(source: string, on: boolean) {
    p.agentKeys = on ? [...p.agentKeys.filter((k) => k !== source), source] : p.agentKeys.filter((k) => k !== source);
  }

  async function pickLogDir() {
    const d = await open({ directory: true, title: "Dossier des journaux de session" });
    if (typeof d === "string") p.termLogDir = d;
  }

  function save() {
    Object.assign(app.prefs, $state.snapshot(p));
    app.savePrefs();
    app.showSettings = false;
  }

  // ---- one-click installation of VcXsrv (winget)
  let xInstalling = $state(false);
  let xProgress = $state("");
  async function installX() {
    if (!(await app.confirm("Installer VcXsrv (serveur X libre) avec winget ? Windows demandera une autorisation d'administrateur."))) return;
    xInstalling = true;
    xProgress = "Lancement de winget…";
    const { listen } = await import("@tauri-apps/api/event");
    const un = await listen<string>("xserver-install", (ev) => (xProgress = ev.payload));
    try {
      app.xserver = await api.xserverInstall();
      xProgress = "";
      app.info("VcXsrv est installé : Orbiterm le démarrera en mode multi-fenêtres.");
    } catch (e) {
      xProgress = "";
      app.alert(e instanceof Error ? e.message : ((e as { message?: string })?.message ?? String(e)));
    } finally {
      un();
      xInstalling = false;
    }
  }

  function openPasswords() {
    app.showSettings = false;
    app.showPasswords = true;
  }

  function reset() {
    // the user's own themes are kept (they are not settings, they are made)
    const keep = { sidebarWidth: p.sidebarWidth, customThemes: p.customThemes };
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
        <label class="chk"><input type="checkbox" bind:checked={p.closeOnEnd} data-close-on-end /> Fermer l'onglet et revenir à l'accueil quand on quitte une session (exit, déconnexion)</label>
        <label class="chk"><input type="checkbox" bind:checked={p.updateCheck} data-update-check /> Rechercher les mises à jour au démarrage (rien n'est installé sans votre accord)</label>
        <div class="row">
          <button
            onclick={() => {
              app.showSettings = false;
              window.dispatchEvent(new Event("orbiterm-update-check"));
            }}
            data-update-now><Icon name="refresh" size={14} /> Rechercher les mises à jour maintenant</button>
        </div>
        <div class="row">
          <label for="defsh">Terminal local par défaut</label>
          <select id="defsh" bind:value={p.defaultShell}>
            <option value="">(premier détecté{app.shells[0] ? ` : ${app.shells[0].label}` : ""})</option>
            {#each app.shells as sh (sh.id)}<option value={sh.id}>{sh.label}</option>{/each}
          </select>
        </div>
      </fieldset>
      <fieldset>
        <legend>Mots de passe</legend>
        <p class="muted">Les mots de passe enregistrés sont conservés dans le trousseau du système.</p>
        <div class="row">
          <button type="button" onclick={openPasswords} data-open-passwords><Icon name="key" size={14} /> Gestion des mots de passe…</button>
        </div>
      </fieldset>
      <fieldset>
        <legend>Données</legend>
        <div class="row">
          <label for="cfg">Dossier de configuration</label>
          <input id="cfg" readonly value={cfgDir} />
          <button onclick={() => clipWrite(cfgDir)} title="Copier le chemin"><Icon name="copy" size={14} /></button>
        </div>
        <p class="muted">Sessions, dossiers, tunnels et macros y sont enregistrés (sessions.toml). Les mots de passe restent dans le trousseau du système.</p>
        <button onclick={() => { app.showSettings = false; app.showImport = true; }}><Icon name="download" size={14} /> Importer des sessions (MobaXterm, PuTTY, OpenSSH)…</button>
      </fieldset>
    {:else if tab === "terminal"}
      <fieldset>
        <legend>Comportement de tous les terminaux</legend>
        <label class="chk" title="Logo de la distribution (Ubuntu, Debian, Fedora, Arch, Windows…) et informations du système : version, noyau, processeur, mémoire, disque, adresse IP. Lu par Orbiterm : rien à installer sur le serveur."><input type="checkbox" bind:checked={p.termFetch} data-term-fetch /> Afficher un résumé du système à l'ouverture d'un shell SSH ou local (logo et informations, façon fastfetch)</label>
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
        <label class="chk" title="error, échec, stopped en rouge ; warning, disabled en ambre ; ok, running, active en vert ; adresses IP et MAC en cyan ; dates et heures en violet ; liens en bleu ; invite du shell (utilisateur, hôte, dossier) quand le serveur ne la colore pas"><input type="checkbox" bind:checked={p.termHighlight} /> Mettre en couleur les mots-clés, adresses, dates et l'invite du shell</label>
      </fieldset>
      <fieldset>
        <legend>Journalisation</legend>
        <label class="chk"><input type="checkbox" bind:checked={p.termLog} /> Enregistrer l’activité de chaque terminal dans un fichier texte</label>
        <div class="row">
          <label for="logdir">Dossier</label>
          <input id="logdir" class="grow" bind:value={p.termLogDir} placeholder="(Documents/Orbiterm/journaux)" disabled={!p.termLog} spellcheck="false" />
          <button type="button" disabled={!p.termLog} onclick={pickLogDir}>…</button>
        </div>
        <p class="muted">Un fichier par session ouverte (nom de la session, date et heure), sans les codes de couleur. Effet à la prochaine ouverture.</p>
      </fieldset>
    {:else if tab === "style"}
      <div class="preview" style={previewStyle} data-style-preview>
        <div><span style:color={preview.green} style:font-weight={boldWeight}>claude@serveur</span>:<span style:color={preview.blue} style:font-weight={boldWeight}>~/projets</span>$ ls</div>
        <div><span style:color={preview.blue}>docs</span>  <span style:color={preview.magenta}>photo.png</span>  <span style:color={preview.green}>deploy.sh</span>  <span style:color={preview.red}>archive.tar.gz</span></div>
        <div><span style:color={preview.red}>erreur :</span> fichier introuvable · <span style:color={preview.yellow}>attention</span> · <span style:color={preview.cyan}>192.168.0.10</span></div>
        <div class="swatches">
          {#each ANSI_KEYS as [k] (k)}<span style:background={preview[k]} title={k}></span>{/each}
        </div>
        <div><span style:background={preview.selectionBackground}>texte sélectionné</span> <span class="cur" style:background={preview.cursor} style:color={preview.cursorAccent}>_</span></div>
      </div>

      <fieldset>
        <legend>Police et couleurs par défaut des nouvelles sessions</legend>
        <div class="grid">
          <label for="tf">Police</label>
          <select id="tf" value={fontOther || !fontKnown ? "__other" : p.termFont} onchange={(e) => pickFont(e.currentTarget.value)} data-font>
            {#each FONTS as f (f.label)}<option value={f.family}>{f.label}{f.installed ? "" : " — non installée"}</option>{/each}
            <option value="__other">Autre police…</option>
          </select>
          <label for="ts">Taille</label>
          <input id="ts" type="number" min="6" max="40" bind:value={p.termFontSize} />
          {#if fontOther || !fontKnown}
            <label for="tfo">Nom de la police</label>
            <input id="tfo" value={firstName(p.termFont)} onchange={(e) => (p.termFont = familyOf(e.currentTarget.value.trim() || "Cascadia Mono"))} spellcheck="false" placeholder="ex. Iosevka Term" />
            <span></span><span></span>
          {/if}
          <label for="tt">Thème</label>
          <select id="tt" bind:value={p.termTheme} data-term-theme>
            {#each themeChoices(p.customThemes) as [id, label] (id)}<option value={id}>{label}</option>{/each}
          </select>
          <label for="tsb">Historique (lignes)</label>
          <input id="tsb" type="number" min="100" max="200000" step="1000" bind:value={p.termScrollback} />
        </div>
        <p class="muted">Les polices « Nerd Font » ajoutent les icônes des invites décorées (Starship, Powerlevel10k…) ; elles s'installent depuis nerdfonts.com.</p>
        <button onclick={applyToAll} disabled={!app.store.sessions.length}>Appliquer à toutes les sessions enregistrées</button>
      </fieldset>

      <fieldset>
        <legend>Espacements et graisse (tous les terminaux)</legend>
        <div class="grid">
          <label for="lh">Interligne</label>
          <span class="range"><input id="lh" type="range" min="1" max="2" step="0.05" bind:value={p.termLineHeight} data-line-height /> {Number(p.termLineHeight).toFixed(2)}</span>
          <label for="ls">Espace entre les lettres</label>
          <span class="range"><input id="ls" type="range" min="0" max="5" step="0.5" bind:value={p.termLetterSpacing} /> {p.termLetterSpacing} px</span>
          <label for="fw">Texte normal</label>
          <select id="fw" bind:value={p.termFontWeight}>{#each WEIGHTS.filter(([w]) => w !== "800") as [w, l] (w)}<option value={w}>{l}</option>{/each}</select>
          <label for="fwb">Texte gras</label>
          <select id="fwb" bind:value={p.termFontWeightBold}>{#each WEIGHTS.filter(([w]) => w !== "300") as [w, l] (w)}<option value={w}>{l}</option>{/each}</select>
        </div>
      </fieldset>

      <fieldset data-themes>
        <legend>Thèmes de couleurs personnalisés</legend>
        {#each p.customThemes as t (t.id)}
          <div class="row theme-row">
            <span class="mini" style:background={t.colors.background} style:color={t.colors.foreground}>Aa</span>
            <span class="grow">{t.name}</span>
            <button class="small" onclick={() => (editingTheme = editingTheme === t.id ? "" : t.id)} data-theme-edit={t.name}>{editingTheme === t.id ? "Fermer" : "Modifier"}</button>
            <button class="small" onclick={() => addTheme(t.id, t.name + " (copie)")}>Dupliquer</button>
            <button class="small" title="Supprimer" aria-label="Supprimer" onclick={() => removeTheme(t.id)}><Icon name="trash" size={14} /></button>
          </div>
          {#if editingTheme === t.id && edited}
            <div class="editor" data-theme-editor>
              <div class="row">
                <label for="tn">Nom</label>
                <input id="tn" bind:value={edited.name} spellcheck="false" />
              </div>
              <div class="colors">
                {#each [...BASE_KEYS, ...ANSI_KEYS] as [k, label] (k)}
                  <label class="color">
                    <input type="color" value={edited.colors[k] ?? "#000000"} oninput={(e) => (edited.colors[k] = e.currentTarget.value)} data-color={k} />
                    <span>{label}</span>
                  </label>
                {/each}
              </div>
              <p class="muted">L'aperçu en haut montre ce thème s'il est choisi comme thème par défaut.</p>
            </div>
          {/if}
        {:else}
          <p class="muted">Aucun thème personnalisé.</p>
        {/each}
        <div class="row">
          <button onclick={() => addTheme(p.termTheme, "Mon thème")} data-theme-new><Icon name="plus" size={14} /> Nouveau thème (à partir du thème choisi)</button>
          <button onclick={() => (importOpen = !importOpen)}>Importer…</button>
        </div>
        {#if importOpen}
          <textarea class="import" bind:value={importText} placeholder={'Colle ici un schéma de couleurs JSON (Windows Terminal : { "name": …, "background": "#…", "foreground": …, "black": … })'} spellcheck="false"></textarea>
          <div class="row"><button onclick={importTheme} disabled={!importText.trim()}>Importer ce thème</button></div>
        {/if}
      </fieldset>

      <fieldset>
        <legend>Image de fond (tous les terminaux)</legend>
        <div class="row">
          <input class="grow" readonly value={p.termBgImage} placeholder="(aucune)" data-bg-path />
          <button onclick={pickBackground} data-bg-pick>Choisir…</button>
          <button onclick={() => (p.termBgImage = "")} disabled={!p.termBgImage}>Retirer</button>
        </div>
        <div class="grid">
          <label for="bgf">Ajustement</label>
          <select id="bgf" bind:value={p.termBgFit} disabled={!p.termBgImage}>
            <option value="cover">Remplir (recadrée)</option>
            <option value="contain">Adapter (entière)</option>
            <option value="center">Centrer (taille réelle)</option>
            <option value="repeat">Mosaïque</option>
          </select>
          <label for="bgo">Voile du thème</label>
          <span class="range"><input id="bgo" type="range" min="0" max="100" step="5" bind:value={p.termBgOpacity} disabled={!p.termBgImage} /> {p.termBgOpacity} %</span>
        </div>
        <p class="muted">Le voile est la couleur de fond du thème posée sur l'image : plus il est opaque, plus le texte reste lisible.</p>
      </fieldset>
    {:else if tab === "ssh"}
      <fieldset>
        <legend>Connexions SSH</legend>
        <div class="row">
          <label for="ka">Maintien de la connexion (keepalive) toutes les</label>
          <input id="ka" class="num" type="number" min="0" max="3600" bind:value={p.keepalive} />
          <span>secondes (0 = désactivé)</span>
        </div>
        <label class="chk" title="Connexion directe ou par rebond, compression, navigateur SFTP, redirection X11 (acceptée ou refusée par le serveur), transfert d'agent"><input type="checkbox" bind:checked={p.sshBanner} /> Afficher un encadré d'accueil en haut du terminal (état de la connexion)</label>
        <label class="chk"><input type="checkbox" bind:checked={p.sshMonitor} /> Afficher les informations du serveur sous le terminal (CPU, mémoire, disque, réseau…)</label>
      </fieldset>
      <fieldset data-agent>
        <legend>Agent SSH</legend>
        <label class="chk"><input type="checkbox" bind:checked={p.sshAgent} /> Se connecter avec les clés des agents avant de demander un mot de passe</label>
        <label class="chk" title="Le serveur pourra utiliser tes clés pendant la session (rebonds vers d'autres machines) : à réserver aux serveurs de confiance"><input type="checkbox" bind:checked={p.agentForward} /> Transférer l'agent aux serveurs (agent forwarding — serveurs de confiance uniquement)</label>
        <p class="muted">Agent du système : {agent ? agent.system || "aucun détecté" : "…"}</p>
        {#if agent?.keys.length}
          <table class="agent">
            <thead><tr><th>Clé</th><th>Origine</th><th title="Charger la clé à chaque démarrage d'Orbiterm">Au démarrage</th><th></th></tr></thead>
            <tbody>
              {#each agent.keys as k (k.fingerprint)}
                <tr data-agent-key={k.comment}>
                  <td title={k.fingerprint}><b>{k.comment || "(sans commentaire)"}</b><br /><span class="mono">{k.algorithm} · {k.fingerprint.slice(0, 22)}…</span></td>
                  <td class="src" title={k.source}>{k.internal ? k.source.split(/[\\/]/).pop() : "agent du système"}</td>
                  <td class="c">{#if k.internal}<input type="checkbox" checked={p.agentKeys.includes(k.source)} onchange={(e) => toggleStartup(k.source, e.currentTarget.checked)} />{/if}</td>
                  <td class="c">{#if k.internal}<button type="button" class="small" title="Retirer de l'agent" aria-label="Retirer" onclick={() => removeAgentKey(k.fingerprint, k.source)}><Icon name="trash" size={14} /></button>{/if}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        {:else if agent}
          <p class="muted">Aucune clé chargée.</p>
        {/if}
        <div class="row">
          <button type="button" onclick={addAgentKey} data-agent-add><Icon name="key" size={14} /> Charger une clé dans l'agent Orbiterm…</button>
        </div>
        <p class="muted">Les clés de l'agent Orbiterm restent en mémoire jusqu'à la fermeture d'Orbiterm ; leur phrase secrète n'est demandée qu'une fois.</p>
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
            {#if app.xserver?.installable}
              <br /><button type="button" onclick={installX} disabled={xInstalling} data-x-install><Icon name="download" size={14} /> Installer VcXsrv…</button>
              {#if xProgress}<br /><span class="muted" data-x-progress>{xProgress}</span>{/if}
            {/if}
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
        <legend>Apparence</legend>
        <div class="row">
          <label for="uit">Thème de l'interface</label>
          <select id="uit" bind:value={p.uiTheme} data-uitheme>
            {#each UI_THEMES as t (t.id)}<option value={t.id}>{t.label}</option>{/each}
          </select>
        </div>
      </fieldset>
      <fieldset>
        <legend>Éléments de la fenêtre</legend>
        <label class="chk"><input type="checkbox" bind:checked={p.menubar} /> Barre de menus</label>
        <label class="chk"><input type="checkbox" bind:checked={p.toolbar} /> Barre d'outils</label>
        <label class="chk"><input type="checkbox" bind:checked={p.sidebar} /> Barre latérale (sessions, outils, macros, SFTP)</label>
        <label class="chk"><input type="checkbox" bind:checked={p.statusbar} /> Barre d'état</label>
        <label class="chk"><input type="checkbox" bind:checked={p.termBar} /> Bandeau au-dessus des terminaux distants (copier, coller, SFTP, fermer la session…)</label>
        <div class="row">
          <label for="sw">Largeur de la barre latérale</label>
          <input id="sw" class="num" type="number" min="180" max="600" bind:value={p.sidebarWidth} /> px
        </div>
      </fieldset>
      <fieldset>
        <legend>Bureaux à distance</legend>
        <label class="chk"><input type="checkbox" bind:checked={p.rdpGpu} data-rdp-gpu /> Dessiner le bureau distant avec la carte graphique (décocher en cas d'image noire)</label>
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
  table.agent {
    width: 100%;
    border-collapse: collapse;
    font-size: 12px;
    margin: 4px 0;
  }
  table.agent th {
    text-align: left;
    font-weight: 600;
    border-bottom: 1px solid var(--chrome-border);
    padding: 2px 4px;
  }
  table.agent td {
    padding: 3px 4px;
    border-bottom: 1px solid var(--row-border);
    vertical-align: middle;
  }
  table.agent .mono {
    font-family: var(--mono, monospace);
    font-size: 11px;
    color: var(--muted);
  }
  table.agent .src {
    max-width: 160px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  table.agent .c {
    text-align: center;
  }
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
  .preview {
    border: 1px solid var(--chrome-border-dark);
    border-radius: 4px;
    padding: 8px 10px;
    margin: 0 0 10px;
    overflow: hidden;
  }
  .preview > div {
    white-space: pre;
  }
  .preview .swatches {
    white-space: normal;
    display: flex;
    gap: 3px;
    margin: 4px 0;
  }
  .preview .swatches span {
    width: 18px;
    height: 12px;
    border-radius: 2px;
  }
  .preview .cur {
    padding: 0 1px;
  }
  .range {
    display: flex;
    align-items: center;
    gap: 6px;
    white-space: nowrap;
  }
  .range input {
    flex: 1;
    min-width: 80px;
  }
  .theme-row .mini {
    width: 34px;
    text-align: center;
    border: 1px solid var(--chrome-border);
    border-radius: 3px;
    font-family: var(--mono, monospace);
    font-size: 11px;
  }
  .small {
    padding: 2px 8px;
    font-size: 12px;
  }
  .editor {
    border: 1px dashed var(--chrome-border-dark);
    padding: 8px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .colors {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 4px 10px;
  }
  .color {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 12px;
  }
  .color input {
    width: 28px;
    height: 20px;
    padding: 0;
    border: 1px solid var(--chrome-border);
  }
  .import {
    min-height: 80px;
    font-family: var(--mono, monospace);
    font-size: 11px;
  }
</style>
