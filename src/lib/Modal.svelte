<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    title,
    children,
    actions,
    onclose,
    width = "460px",
  }: {
    title: string;
    children: Snippet;
    actions: Snippet;
    onclose?: () => void;
    width?: string;
  } = $props();

  function key(e: KeyboardEvent) {
    if (e.key === "Escape") onclose?.();
  }
</script>

<svelte:window onkeydown={key} />

<div class="backdrop" role="presentation">
  <div class="modal" role="dialog" aria-modal="true" aria-label={title} style:width>
    <header>{title}</header>
    <div class="body">{@render children()}</div>
    <footer>{@render actions()}</footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }
  .modal {
    max-width: calc(100vw - 32px);
    max-height: calc(100vh - 32px);
    display: flex;
    flex-direction: column;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.5);
  }
  header {
    padding: 12px 16px;
    font-weight: 600;
    border-bottom: 1px solid var(--border);
  }
  .body {
    padding: 16px;
    overflow: auto;
  }
  footer {
    padding: 12px 16px;
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    border-top: 1px solid var(--border);
  }
</style>
