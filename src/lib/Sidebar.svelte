<script lang="ts">
  import Icon from "./Icon.svelte";
  import SessionTree from "./SessionTree.svelte";
  import SftpPanel from "./SftpPanel.svelte";
  import { app, type SideTab } from "./app.svelte";

  let filter = $state("");
  let quick = $state("");

  const sideTabs: Array<{ id: SideTab; label: string; icon: string }> = $derived([
    { id: "sessions", label: "Sessions", icon: "star" },
    { id: "tools", label: "Outils", icon: "tools" },
    { id: "macros", label: "Macros", icon: "macro" },
    ...(app.hasSftp ? [{ id: "sftp" as const, label: "Sftp", icon: "sftp" }] : []),
  ]);

  $effect(() => {
    if (!app.hasSftp && app.sideTab === "sftp") app.sideTab = "sessions";
  });

  function pick(id: SideTab) {
    if (app.sideTab === id && app.prefs.sidebar) {
      app.prefs.sidebar = false; // clicking the current tab folds the panel (like the original)
    } else {
      app.sideTab = id;
      app.prefs.sidebar = true;
    }
    app.savePrefs();
  }

  function doQuick(e: SubmitEvent) {
    e.preventDefault();
    if (!quick.trim()) return;
    app.quickConnect(quick);
    quick = "";
  }

  type Tool = { label: string; icon: string; action?: () => void; title?: string };
  const tools: Array<{ group: string; items: Tool[] }> = $derived([
    {
      group: "Terminaux locaux",
      items: app.shells.map((sh) => ({
        label: sh.label,
        icon: sh.id.startsWith("wsl:") ? "wsl" : "shell",
        title: `${sh.program} ${sh.args.join(" ")}`,
        action: () => app.startLocalTerminal(sh.id),
      })),
    },
    {
      group: "Réseau",
      items: [
        { label: "Scanner de ports", icon: "scan", action: () => (app.netTool = "scan") },
        { label: "Ports ouverts / connexions", icon: "ports", action: () => (app.netTool = "ports") },
        { label: "Wake-on-LAN", icon: "wol", action: () => (app.netTool = "wol") },
        {
          label: "Gestionnaire de tunnels SSH",
          icon: "tunnel",
          action: () => {
            app.refreshTunnels();
            app.showTunnels = true;
          },
        },
        {
          label: app.xserver?.running ? "Arrêter le serveur X" : "Démarrer le serveur X",
          icon: app.xserver?.running ? "xserver" : "xserver_off",
          action: () => app.toggleX(),
        },
      ],
    },
    {
      group: "Serveurs intégrés",
      items: [
        ...(
          [
            ["http", "Serveur HTTP", "web"],
            ["tftp", "Serveur TFTP", "network"],
            ["ftp", "Serveur FTP", "ftp"],
            ["ssh", "Serveur SSH/SFTP", "ssh"],
          ] as const
        ).map(([k, l, icon]) => {
          const s = app.servers.find((x) => x.config.kind === k);
          return { label: s?.running ? `${l} (actif, port ${s.port})` : l, icon, action: () => app.openServers(k) };
        }),
      ],
    },
    {
      group: "Orbiterm",
      items: [
        { label: "Importer des sessions…", icon: "download", action: () => (app.showImport = true) },
        { label: "Configuration…", icon: "settings", action: () => (app.showSettings = true) },
      ],
    },
  ]);
</script>

<div class="sidebar" class:folded={!app.prefs.sidebar}>
  <div class="vtabs" role="tablist" aria-orientation="vertical">
    {#each sideTabs as t (t.id)}
      <button
        class="vtab"
        role="tab"
        aria-selected={app.sideTab === t.id && app.prefs.sidebar}
        class:on={app.sideTab === t.id && app.prefs.sidebar}
        data-sidetab={t.id}
        onclick={() => pick(t.id)}
      >
        <Icon name={t.icon} size={14} />
        <span>{t.label}</span>
      </button>
    {/each}
  </div>

  {#if app.prefs.sidebar}
    <div class="panel" style:width="{app.prefs.sidebarWidth}px">
      <div class="body" class:hide={app.sideTab !== "sessions"}>
        <form class="quick" onsubmit={doQuick}>
          <Icon name="search" size={15} />
          <input bind:value={quick} placeholder="Connexion rapide…" spellcheck="false" aria-label="Connexion rapide" data-quick />
        </form>
        <div class="minibar">
          <button title="Nouvelle session" onclick={() => app.newSession()}><Icon name="session" size={16} /></button>
          <button title="Nouveau dossier" onclick={() => app.newFolder("")}><Icon name="newfolder" size={16} /></button>
          <button title="Terminal local" onclick={() => app.startLocalTerminal()}><Icon name="terminal" size={16} /></button>
          <button title="Recharger la liste" onclick={() => app.reload()}><Icon name="refresh" size={16} /></button>
          <input class="filter" bind:value={filter} placeholder="Filtrer" aria-label="Filtrer les sessions" />
        </div>
        <div class="scroll"><SessionTree {filter} /></div>
      </div>

      <div class="body" class:hide={app.sideTab !== "tools"}>
        <div class="scroll">
          {#each tools as g (g.group)}
            <div class="group">{g.group}</div>
            {#each g.items as it (it.label)}
              <button
                class="tool"
                disabled={!it.action}
                title={it.action ? (it.title ?? it.label) : `${it.label} — prochaine étape`}
                onclick={it.action}
              >
                <Icon name={it.icon} size={18} />
                <span>{it.label}</span>
              </button>
            {/each}
          {/each}
        </div>
      </div>

      <div class="body" class:hide={app.sideTab !== "macros"}>
        <div class="minibar">
          {#if app.recording}
            <button class="rec on" title="Arrêter l'enregistrement" data-macro-stop onclick={() => app.stopRecording()}>
              <Icon name="stop" size={16} /> <span>Arrêter</span>
            </button>
            <span class="recinfo">● {app.recording.steps.length} frappe(s)</span>
          {:else}
            <button class="rec" title="Enregistrer les frappes du terminal actif" data-macro-record onclick={() => app.startRecording()}>
              <Icon name="record" size={16} /> <span>Enregistrer</span>
            </button>
          {/if}
        </div>
        <div class="scroll">
          {#each app.store.macros as m (m.id)}
            <div class="macro" data-macro={m.name}>
              <button class="play" title="Rejouer {app.multiExec ? 'dans les terminaux MultiExec' : 'dans le terminal actif'}" aria-label="Rejouer" disabled={app.playing} onclick={() => app.playMacro(m)}>
                <Icon name="play" size={14} />
              </button>
              <span class="mname" title="{m.steps.length} étape(s)">{m.name}</span>
              <button class="mini" title="Renommer" aria-label="Renommer" onclick={() => app.renameMacro(m)}><Icon name="edit" size={13} /></button>
              <button class="mini" title="Supprimer" aria-label="Supprimer" onclick={() => app.deleteMacro(m)}><Icon name="trash" size={13} /></button>
            </div>
          {:else}
            <p class="muted pad">
              Aucune macro. Clique sur « Enregistrer », tape tes commandes dans le terminal actif, puis « Arrêter ». La macro
              pourra être rejouée dans le terminal actif, ou dans tous les terminaux en mode MultiExec.
            </p>
          {/each}
        </div>
      </div>

      <div class="body" class:hide={app.sideTab !== "sftp"}>
        {#each app.tabs as t (t.key)}
          {#if t.connId && t.state === "open" && t.session.kind === "ssh" && t.session.sftp_enabled}
            <div class="fill" class:hide={t.key !== app.activeKey}>
              <SftpPanel
                connId={t.connId}
                visible={app.sideTab === "sftp" && t.key === app.activeKey}
                askText={app.askText}
                confirm={app.confirm}
                onseparate={() => app.openSeparateSftp(t)}
              />
            </div>
          {/if}
        {/each}
      </div>
    </div>
  {/if}
</div>

<style>
  .sidebar {
    display: flex;
    min-height: 0;
    height: 100%;
    background: var(--panel);
  }
  .vtabs {
    display: flex;
    flex-direction: column;
    width: 24px;
    background: var(--chrome);
    border-right: 1px solid var(--chrome-border);
    padding-top: 2px;
  }
  .vtab {
    writing-mode: vertical-rl;
    transform: rotate(180deg);
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 10px 0;
    margin: 1px 0;
    border: 1px solid transparent;
    border-radius: 0;
    background: none;
    color: var(--text);
    font-size: 12px;
    width: 23px;
  }
  .vtab :global(svg) {
    transform: rotate(90deg);
  }
  .vtab:hover {
    background: var(--hover);
  }
  .vtab.on {
    background: var(--panel);
    border-color: var(--chrome-border);
    border-left-color: var(--panel);
    font-weight: 600;
    box-shadow: inset -3px 0 0 var(--accent);
  }
  .panel {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  .body {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .fill {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .quick {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 4px;
    padding: 0 6px;
    border: 1px solid var(--chrome-border-dark);
    background: var(--input-bg);
  }
  .quick input {
    flex: 1;
    border: none;
    outline: none;
    background: none;
    padding: 4px 0;
  }
  .minibar {
    display: flex;
    align-items: center;
    gap: 1px;
    padding: 0 4px 3px;
    border-bottom: 1px solid var(--chrome-border);
  }
  .minibar button {
    padding: 2px 3px;
    border: 1px solid transparent;
    border-radius: 2px;
    background: none;
    display: flex;
  }
  .minibar button:hover {
    background: var(--hover);
    border-color: var(--hover-border);
  }
  .filter {
    flex: 1;
    margin-left: 4px;
    min-width: 40px;
    padding: 2px 5px;
    font-size: 12px;
  }
  .scroll {
    flex: 1;
    overflow: auto;
    min-height: 0;
  }
  .group {
    font-weight: 600;
    padding: 8px 8px 3px;
    color: var(--brand);
    border-bottom: 1px solid var(--chrome-border);
    margin-bottom: 2px;
  }
  .tool {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 3px 10px;
    border: 1px solid transparent;
    border-radius: 0;
    background: none;
    text-align: left;
  }
  .tool:hover:not(:disabled) {
    background: var(--hover);
    border-color: var(--hover-border);
  }
  .tool:disabled {
    color: var(--text-disabled);
    cursor: default;
  }
  .tool:disabled :global(svg) {
    opacity: 0.5;
  }
  .pad {
    padding: 12px;
  }
  .minibar .rec {
    gap: 4px;
    padding: 2px 6px;
    margin-top: 4px;
  }
  .rec.on {
    color: var(--danger);
  }
  .recinfo {
    color: var(--danger);
    font-size: 12px;
    margin-left: 6px;
    margin-top: 4px;
  }
  .macro {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 2px 6px;
  }
  .macro:hover {
    background: var(--hover);
  }
  .mname {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .macro button {
    padding: 2px 4px;
    border-color: transparent;
    background: none;
  }
  .macro .play {
    color: var(--ok);
  }
</style>
