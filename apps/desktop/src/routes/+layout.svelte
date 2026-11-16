<script lang="ts">
  import { isDemo } from "$lib/demo";
  import "@fontsource/barlow/400.css";
  import "@fontsource/barlow/500.css";
  import "@fontsource/barlow/600.css";
  import "@fontsource/barlow-semi-condensed/500.css";
  import "@fontsource/barlow-semi-condensed/600.css";
  import "$lib/theme.css";
  import "$lib/glass.css";
  // Applies the saved look to <html> before the first paint.
  import { look, LOOKS, type Look } from "$lib/look.svelte";

  let { children } = $props();

  // The website's Glass / Classic switch, next to the demo, changes the demo's look too.
  function onMessage(e: MessageEvent) {
    if (e.source !== window.parent || e.data?.type !== "bitspring-look") return;
    if (LOOKS.includes(e.data.look)) look.value = e.data.look as Look;
  }
</script>

<svelte:window onmessage={isDemo ? onMessage : undefined} />

{@render children()}
