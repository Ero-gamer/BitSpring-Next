<script lang="ts">
  // The heart: outlined while the track or album is not a favourite, filled once it is.
  let { on = false, size = 16 }: { on?: boolean; size?: number } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 22 22" aria-hidden="true">
  <path d="M11 18.2L4.6 11.6A4 4 0 0 1 11 6.8a4 4 0 0 1 6.4 4.8z" fill={on ? "currentColor" : "none"} stroke="currentColor" stroke-width="1.5" stroke-linejoin="round" />
</svg>
