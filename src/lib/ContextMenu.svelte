<script lang="ts">
  import Icon from "./Icon.svelte";
  import { app } from "./app.svelte";

  let el: HTMLDivElement | undefined = $state();
  let pos = $state({ x: 0, y: 0 });

  // keep the menu inside the window
  $effect(() => {
    const c = app.ctx;
    if (!c || !el) return;
    const r = el.getBoundingClientRect();
    pos = {
      x: Math.min(c.x, window.innerWidth - r.width - 4),
      y: Math.min(c.y, window.innerHeight - r.height - 4),
    };
  });
</script>

<svelte:window
  onpointerdown={(e) => {
    if (app.ctx && !(e.target as HTMLElement).closest(".ctx")) app.ctx = null;
  }}
  onkeydown={(e) => e.key === "Escape" && (app.ctx = null)}
  onblur={() => (app.ctx = null)}
/>

{#if app.ctx}
  <div class="ctx" role="menu" bind:this={el} style:left="{pos.x || app.ctx.x}px" style:top="{pos.y || app.ctx.y}px">
    {#each app.ctx.items as it, i (i)}
      {#if it.sep}
        <div class="sep"></div>
      {:else}
        <button
          class="item"
          role="menuitem"
          disabled={it.disabled}
          onclick={() => {
            app.ctx = null;
            it.action?.();
          }}
        >
          <span class="ic">{#if it.checked}✓{:else if it.icon}<Icon name={it.icon} size={16} />{/if}</span>
          <span class="lb">{it.label}</span>
          <span class="sc">{it.shortcut ?? ""}</span>
        </button>
      {/if}
    {/each}
  </div>
{/if}

<style>
  .ctx {
    position: fixed;
    z-index: 300;
    min-width: 220px;
    background: var(--panel);
    border: 1px solid var(--chrome-border-dark);
    box-shadow: 0 4px 14px var(--shadow);
    border-radius: 6px;
    padding: 2px;
    user-select: none;
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
  }
  .ic {
    width: 22px;
    display: flex;
    justify-content: center;
  }
  .lb {
    flex: 1;
    white-space: nowrap;
  }
  .sc {
    color: var(--muted);
    font-size: 11.5px;
  }
  .sep {
    height: 1px;
    margin: 3px 4px 3px 32px;
    background: var(--chrome-border);
  }
</style>
