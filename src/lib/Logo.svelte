<script lang="ts">
  // Orbiterm logo: a terminal prompt on a planet with an orbit ring.
  let { size = 32 }: { size?: number } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 48 48" aria-hidden="true">
  <defs>
    <linearGradient id="orb-g" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="#3d8be0" />
      <stop offset="1" stop-color="#1b4f8f" />
    </linearGradient>
  </defs>
  <circle cx="24" cy="24" r="15" fill="url(#orb-g)" />
  <path d="M17 19l5 5-5 5M24 30h8" stroke="#fff" stroke-width="3" fill="none" stroke-linecap="round" stroke-linejoin="round" />
  <ellipse cx="24" cy="24" rx="22" ry="7.5" fill="none" stroke="#f2a43a" stroke-width="2.5" transform="rotate(-25 24 24)" />
  <circle cx="42.5" cy="15.5" r="3" fill="#f2a43a" />
</svg>
