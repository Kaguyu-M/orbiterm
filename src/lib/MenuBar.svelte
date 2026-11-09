<script lang="ts" module>
  export type MenuItem =
    | { sep: true }
    | {
        sep?: false;
        label: string;
        icon?: string;
        shortcut?: string;
        disabled?: boolean;
        checked?: boolean;
        action?: () => void;
      };
  export type Menu = { label: string; items: () => MenuItem[] };
</script>

<script lang="ts">
  import Icon from "./Icon.svelte";

  let { menus }: { menus: Menu[] } = $props();
  let open: number | null = $state(null);

  function run(it: MenuItem) {
    if (it.sep || it.disabled) return;
    open = null;
    it.action?.();
  }
</script>

<svelte:window
  onpointerdown={(e) => {
    if (open !== null && !(e.target as HTMLElement).closest(".menubar")) open = null;
  }}
  onkeydown={(e) => e.key === "Escape" && (open = null)}
/>

<nav class="menubar">
  {#each menus as m, i (m.label)}
    <div class="menu">
      <button
        class="top"
        class:on={open === i}
        onclick={() => (open = open === i ? null : i)}
        onpointerenter={() => open !== null && (open = i)}>{m.label}</button
      >
      {#if open === i}
        <div class="drop" role="menu">
          {#each m.items() as it, j (j)}
            {#if it.sep}
              <div class="sep"></div>
            {:else}
              <button class="item" role="menuitem" disabled={it.disabled} onclick={() => run(it)}>
                <span class="ic">
                  {#if it.checked}<span class="check">✓</span>{:else if it.icon}<Icon name={it.icon} size={16} />{/if}
                </span>
                <span class="lb">{it.label}</span>
                <span class="sc">{it.shortcut ?? ""}</span>
              </button>
            {/if}
          {/each}
        </div>
      {/if}
    </div>
  {/each}
</nav>

<style>
  .menubar {
    display: flex;
    background: var(--chrome);
    border-bottom: 1px solid var(--chrome-border);
    height: 22px;
    padding-left: 2px;
    user-select: none;
  }
  .menu {
    position: relative;
  }
  .top {
    height: 22px;
    padding: 0 8px;
    border: 1px solid transparent;
    border-radius: 0;
    background: none;
    color: var(--text);
  }
  .top:hover,
  .top.on {
    background: var(--hover);
    border-color: var(--hover-border);
  }
  .drop {
    position: absolute;
    left: 0;
    top: 22px;
    z-index: 200;
    min-width: 250px;
    background: var(--panel);
    border: 1px solid var(--chrome-border-dark);
    box-shadow: 2px 3px 8px rgba(0, 0, 0, 0.25);
    padding: 2px;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: 24px;
    padding: 0 10px 0 4px;
    border: 1px solid transparent;
    border-radius: 0;
    background: none;
    text-align: left;
    color: var(--text);
  }
  .item:hover:not(:disabled) {
    background: var(--hover);
    border-color: var(--hover-border);
  }
  .item:disabled {
    color: var(--text-disabled);
    cursor: default;
  }
  .ic {
    width: 22px;
    display: flex;
    justify-content: center;
  }
  .check {
    font-weight: bold;
  }
  .lb {
    flex: 1;
    white-space: nowrap;
  }
  .sc {
    color: var(--muted);
    font-size: 11.5px;
    white-space: nowrap;
  }
  .sep {
    height: 1px;
    margin: 3px 4px 3px 32px;
    background: var(--chrome-border);
  }
</style>
