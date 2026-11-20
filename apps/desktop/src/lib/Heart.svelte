<script lang="ts">
  // The heart: outlined while the track or album is not a favourite, filled once it is.
  //
  // `beats`: where the heart is the listener's own button, pressing it is answered — the
  // heart beats once and its outline goes out from it like a wave off a speaker cone; taken
  // back, it gives a little. Only a change made in front of the listener moves: a heart drawn
  // already on (a list scrolled into view, another track on the player) stays still, so
  // whoever shows a different thing in the same place wraps this in a {#key}.
  import { untrack } from "svelte";

  let { on = false, size = 16, beats = false }: { on?: boolean; size?: number; beats?: boolean } = $props();

  // The heart is "M11 18.2L4.6 11.6A4 4 0 0 1 11 6.8a4 4 0 0 1 6.4 4.8z" under a 1.5 line, but
  // drawn as fills: FULL is that line's outer edge, HOLE its inner one, and the outlined heart
  // is the ring between them. Not a stroke on purpose: in a track list, Android's WebView
  // (seen on Chrome 124) answered the stroked curve by painting other things wrong, the
  // header's Enhancers button gone and rows drawn a second time over the header.
  const FULL = "M10.46 18.72L4.06 12.12L4 12.05A4.75 4.75 0 0 1 11 5.69A4.75 4.75 0 0 1 18 12.05L17.94 12.12L11.54 18.72A.75 .75 0 0 1 10.46 18.72Z";
  const HOLE = "M11 17.12L5.17 11.11A3.25 3.25 0 0 1 10.4 7.25A.75 .75 0 0 0 11.6 7.25A3.25 3.25 0 0 1 16.83 11.11Z";
  const RING = FULL + HOLE;

  let move = $state<"beat" | "give" | null>(null);
  let was = untrack(() => on);
  $effect(() => {
    if (on !== was) {
      was = on;
      if (beats) move = on ? "beat" : "give";
    }
  });
</script>

<svg class:beat={move === "beat"} class:give={move === "give"} width={size} height={size} viewBox="0 0 22 22" aria-hidden="true" onanimationend={(e) => e.target === e.currentTarget && (move = null)}>
  {#if move === "beat"}<path class="echo" d={RING} fill="currentColor" fill-rule="evenodd" />{/if}
  <path d={on ? FULL : RING} fill="currentColor" fill-rule="evenodd" />
</svg>

<style>
  svg {
    overflow: visible;
  }
  /* (At rest nothing is transformed: a window that doesn't run animations shows the plain heart.) */
  .beat {
    animation: beat 440ms cubic-bezier(0.3, 0, 0.2, 1);
  }
  .give {
    animation: give 220ms ease-out;
  }
  .echo {
    opacity: 0;
    transform-box: fill-box;
    transform-origin: center;
    animation: echo 440ms cubic-bezier(0.2, 0.6, 0.3, 1);
    pointer-events: none;
  }
  @keyframes beat {
    0% {
      transform: scale(0.72);
    }
    42% {
      transform: scale(1.24);
    }
    72% {
      transform: scale(0.95);
    }
    100% {
      transform: scale(1);
    }
  }
  @keyframes give {
    0% {
      transform: scale(1);
    }
    45% {
      transform: scale(0.84);
    }
    100% {
      transform: scale(1);
    }
  }
  @keyframes echo {
    0% {
      opacity: 0.6;
      transform: scale(1);
    }
    100% {
      opacity: 0;
      transform: scale(1.85);
    }
  }
</style>
