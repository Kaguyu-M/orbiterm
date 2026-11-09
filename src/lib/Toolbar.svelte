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
      <Icon name={b.icon} size={30} />
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
    gap: 1px;
    padding: 2px 4px;
    background: linear-gradient(var(--toolbar-top), var(--toolbar-bottom));
    border-bottom: 1px solid var(--chrome-border);
    user-select: none;
    overflow: hidden;
  }
  .tb {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 2px;
    min-width: 58px;
    height: 54px;
    padding: 2px 6px;
    border: 1px solid transparent;
    border-radius: 3px;
    background: none;
    color: var(--text);
    font-size: 11.5px;
  }
  .tb:hover:not(:disabled),
  .tb.active {
    background: var(--hover);
    border-color: var(--hover-border);
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
    margin: 6px 3px;
    background: var(--chrome-border);
  }
  .grow {
    flex: 1;
  }
</style>
