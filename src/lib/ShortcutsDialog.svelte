<script lang="ts">
  // Aide → Raccourcis clavier: every action with its shortcuts, changeable. "+" records the
  // next key press; a shortcut already used by another action is moved (the other loses it).
  import Modal from "./Modal.svelte";
  import { app } from "./app.svelte";
  import { IS_WINDOWS } from "./api";
  import { ACTIONS, comboOf, keysOf, label, refusal, type ActionId } from "./shortcuts";

  let { onclose }: { onclose: () => void } = $props();

  let recording: ActionId | null = $state(null);
  let note = $state("");

  const nameOf = (id: ActionId) => ACTIONS.find((a) => a.id === id)?.label ?? id;
  const isDefault = (id: ActionId) => !app.prefs.keys[id];

  function save(id: ActionId, keys: string[]) {
    const def = ACTIONS.find((a) => a.id === id)!.keys;
    const next = { ...app.prefs.keys };
    if (keys.length === def.length && keys.every((k, i) => k === def[i])) delete next[id];
    else next[id] = keys;
    app.prefs.keys = next;
    app.savePrefs();
  }

  function remove(id: ActionId, combo: string) {
    note = "";
    save(id, keysOf(app.prefs.keys, id).filter((k) => k !== combo));
  }

  function reset(id: ActionId) {
    note = "";
    const next = { ...app.prefs.keys };
    delete next[id];
    app.prefs.keys = next;
    // a default shortcut the user had given to another action stays with that one
    app.savePrefs();
  }

  function resetAll() {
    note = "";
    recording = null;
    app.prefs.keys = {};
    app.savePrefs();
  }

  function start(id: ActionId) {
    note = "";
    recording = recording === id ? null : id;
  }

  // the key press being recorded never reaches the rest of the application (nor the dialog's
  // Escape, which here only cancels the recording)
  function onKey(e: KeyboardEvent) {
    if (!recording) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.code === "Escape" && !e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey) {
      recording = null;
      return;
    }
    const combo = comboOf(e);
    if (!combo) return; // a modifier alone: wait for the key
    const why = refusal(combo);
    if (why) {
      note = why;
      return;
    }
    const id = recording;
    recording = null;
    const moved: string[] = [];
    for (const a of ACTIONS) {
      if (a.id === id) continue;
      const ks = keysOf(app.prefs.keys, a.id);
      if (ks.includes(combo)) {
        save(a.id, ks.filter((k) => k !== combo));
        moved.push(a.label);
      }
    }
    const mine = keysOf(app.prefs.keys, id);
    if (!mine.includes(combo)) save(id, [...mine, combo]);
    note = moved.length ? `${label(combo, IS_WINDOWS)} retiré de « ${moved.join(" », « ")} ».` : "";
  }

  $effect(() => {
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  });
</script>

<Modal title="Raccourcis clavier" onclose={() => (recording ? (recording = null) : onclose())} width="620px">
  <table class="sc" data-shortcuts>
    <tbody>
      {#each ACTIONS as a (a.id)}
        <tr class:rec={recording === a.id}>
          <td class="name">{a.label}</td>
          <td class="keys">
            {#each keysOf(app.prefs.keys, a.id) as k (k)}
              <span class="chip">
                {label(k, IS_WINDOWS)}
                <button class="x" title="Retirer ce raccourci" aria-label="Retirer {label(k, IS_WINDOWS)}" onclick={() => remove(a.id, k)}>×</button>
              </span>
            {/each}
            {#if recording === a.id}
              <span class="wait">Appuyez sur la combinaison… (Échap : annuler)</span>
            {:else if !keysOf(app.prefs.keys, a.id).length}
              <span class="none">aucun</span>
            {/if}
          </td>
          <td class="act">
            <button class="mini" title="Ajouter un raccourci" onclick={() => start(a.id)} data-sc-add={a.id}>{recording === a.id ? "Annuler" : "Ajouter"}</button>
            <button class="mini" title="Raccourcis d'origine" disabled={isDefault(a.id)} onclick={() => reset(a.id)}>Par défaut</button>
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
  {#if note}<p class="note" data-sc-note>{note}</p>{/if}
  <p class="fixed">
    Fixes : Ctrl+S / Ctrl+F / Ctrl+G dans l'éditeur · sélection = copie, clic droit = coller dans les terminaux ·
    {IS_WINDOWS ? "bureau à distance en plein écran : Ctrl+Alt+Fin = Ctrl+Alt+Suppr, Ctrl+Alt+Pause quitte le plein écran" : "bureau à distance en plein écran : Ctrl+Alt+Pause quitte le plein écran"}.
  </p>
  {#snippet actions()}
    <button class="dlg" onclick={resetAll} disabled={!Object.keys(app.prefs.keys).length}>Tout rétablir</button>
    <button class="primary dlg" onclick={onclose}>OK</button>
  {/snippet}
</Modal>

<style>
  table.sc {
    width: 100%;
    border-collapse: collapse;
    font-size: 12.5px;
  }
  table.sc td {
    padding: 4px 6px;
    border-bottom: 1px solid var(--row-border);
    vertical-align: middle;
  }
  tr.rec td {
    background: color-mix(in srgb, var(--accent) 14%, transparent);
  }
  .name {
    white-space: nowrap;
  }
  .keys {
    width: 100%;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    margin: 1px 4px 1px 0;
    padding: 1px 2px 1px 6px;
    border: 1px solid var(--chrome-border);
    border-radius: 4px;
    font-family: var(--mono);
    font-size: 11.5px;
    white-space: nowrap;
  }
  .chip .x {
    border: 0;
    background: none;
    padding: 0 3px;
    min-height: 0;
    line-height: 1;
    cursor: pointer;
    color: var(--muted);
  }
  .chip .x:hover {
    color: var(--accent);
  }
  .wait {
    color: var(--accent);
    font-style: italic;
  }
  .none {
    color: var(--muted);
  }
  .act {
    white-space: nowrap;
  }
  .mini {
    font-size: 11.5px;
    padding: 1px 6px;
  }
  .note {
    margin: 8px 0 0;
    color: var(--accent);
    font-size: 12px;
  }
  .fixed {
    margin: 8px 0 0;
    color: var(--muted);
    font-size: 11.5px;
  }
</style>
