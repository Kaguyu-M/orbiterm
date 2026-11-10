<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    title,
    children,
    actions,
    onclose,
    width = "460px",
    align = "center",
  }: {
    title: string;
    children: Snippet;
    actions: Snippet;
    onclose?: () => void;
    width?: string;
    align?: "center" | "end";
  } = $props();

  function key(e: KeyboardEvent) {
    if (e.key === "Escape") onclose?.();
  }
</script>

<svelte:window onkeydown={key} />

<div class="backdrop" role="presentation">
  <div class="modal" role="dialog" aria-modal="true" aria-label={title} style:width>
    <header>
      <span>{title}</span>
      {#if onclose}<button class="close" aria-label="Fermer" onclick={onclose}>✕</button>{/if}
    </header>
    <div class="body">{@render children()}</div>
    <footer style:justify-content={align}>{@render actions()}</footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: var(--backdrop);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }
  .modal {
    max-width: calc(100vw - 24px);
    max-height: calc(100vh - 24px);
    display: flex;
    flex-direction: column;
    background: var(--panel);
    border: 1px solid var(--chrome-border);
    border-radius: 10px;
    overflow: hidden;
    box-shadow: 0 12px 40px var(--shadow);
  }
  header {
    display: flex;
    align-items: center;
    height: 36px;
    padding-left: 14px;
    font-weight: 600;
    background: var(--panel);
    color: var(--text);
    border-bottom: 1px solid var(--chrome-border);
  }
  header span {
    flex: 1;
  }
  .close {
    width: 44px;
    height: 36px;
    border: none;
    border-radius: 0;
    background: none;
    color: inherit;
  }
  .close:hover {
    background: var(--danger);
    color: #fff;
  }
  .body {
    padding: 14px 16px;
    overflow: auto;
  }
  footer {
    padding: 10px 16px;
    display: flex;
    gap: 8px;
    border-top: 1px solid var(--chrome-border);
    background: var(--chrome);
  }
</style>
