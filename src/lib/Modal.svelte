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
    background: rgba(0, 0, 0, 0.3);
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
    border: 1px solid var(--chrome-border-dark);
    box-shadow: 0 8px 30px rgba(0, 0, 0, 0.35);
  }
  header {
    display: flex;
    align-items: center;
    height: 30px;
    padding-left: 10px;
    background: var(--titlebar);
    color: var(--titlebar-text);
    border-bottom: 1px solid var(--chrome-border);
  }
  header span {
    flex: 1;
  }
  .close {
    width: 40px;
    height: 30px;
    border: none;
    border-radius: 0;
    background: none;
    color: inherit;
  }
  .close:hover {
    background: #e81123;
    color: #fff;
  }
  .body {
    padding: 12px 14px;
    overflow: auto;
  }
  footer {
    padding: 10px 14px;
    display: flex;
    gap: 8px;
    border-top: 1px solid var(--chrome-border);
    background: var(--chrome);
  }
</style>
