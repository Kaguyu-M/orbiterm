<script lang="ts" module>
  export type ToolButton =
    | { sep: true }
    | { sep?: false; id: string; label: string; icon: string; title?: string; disabled?: boolean; active?: boolean; action: (ev: MouseEvent) => void };
</script>

<script lang="ts">
  import Icon from "./Icon.svelte";
  let { left, right }: { left: ToolButton[]; right: ToolButton[] } = $props();
</script>

{#snippet btn(b: ToolButton)}
  {#if b.sep}
    <div class="sep"></div>
  {:else}
    <button class="tb" class:active={b.active} data-tb={b.id} disabled={b.disabled} title={b.title ?? b.label} onclick={b.action}>
      <Icon name={b.icon} size={22} />
      <span>{b.label}</span>
    </button>
  {/if}
{/snippet}

<div class="toolbar">
  {#each left as b, i (i)}{@render btn(b)}{/each}
  <div class="grow"></div>
  {#each right as b, i (i)}{@render btn(b)}{/each}
</div>

<style>
  .toolbar {
    display: flex;
    align-items: stretch;
    gap: 2px;
    padding: 3px 6px;
    background: var(--chrome);
    border-bottom: 1px solid var(--chrome-border);
    user-select: none;
    overflow: hidden;
  }
  .tb {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 1px;
    min-width: 54px;
    height: 44px;
    padding: 2px 6px;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--text);
    font-size: 11px;
  }
  .tb:hover:not(:disabled) {
    background: var(--hover);
  }
  .tb.active {
    background: var(--selected);
  }
  .tb:disabled {
    opacity: 0.45;
    cursor: default;
  }
  .tb span {
    white-space: nowrap;
  }
  .sep {
    width: 1px;
    margin: 10px 4px;
    background: var(--chrome-border);
  }
  .grow {
    flex: 1;
  }
</style>
