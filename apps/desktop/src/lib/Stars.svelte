<script lang="ts">
  // A track's rating: five stars. A click on star N gives N; a click on the star that is the
  // rating already takes the stars away.
  import { num, t } from "./i18n.svelte";

  let {
    value = 0,
    onchange,
    label,
    large = false,
  }: {
    value?: number;
    onchange: (rating: number) => void;
    /** Whose rating it is, for screen readers. */
    label: string;
    /** Stars a finger can hit (the player); the small ones are for the rows of a list. */
    large?: boolean;
  } = $props();
  let hover = $state(0);
</script>

<span class="stars" class:rated={value > 0} class:large role="group" aria-label={label} dir="ltr" onmouseleave={() => (hover = 0)}>
  {#each [1, 2, 3, 4, 5] as n (n)}
    <button
      class:lit={n <= (hover || value)}
      class:dim={hover > 0 && n <= value && n > hover}
      onmouseenter={() => (hover = n)}
      onclick={(e) => (e.stopPropagation(), onchange(n === value ? 0 : n), (hover = 0))}
      ondblclick={(e) => e.stopPropagation()}
      aria-label={n === value ? t("rate.clear") : t("rate.give", { n: num(n) })}
      title={n === value ? t("rate.clear") : undefined}
      aria-pressed={n === value}
    >
      <svg width={large ? 19 : 13} height={large ? 19 : 13} viewBox="0 0 14 14" aria-hidden="true">
        <path d="M7 1.3l1.7 3.6 3.9.5-2.9 2.7.8 3.9L7 10.1 3.5 12l.8-3.9L1.4 5.4l3.9-.5z" fill="currentColor" stroke="currentColor" stroke-width="1" stroke-linejoin="round" />
      </svg>
    </button>
  {/each}
</span>

<style>
  .stars {
    display: inline-flex;
    vertical-align: middle;
  }
  button {
    display: grid;
    place-items: center;
    width: 17px;
    height: 22px;
    padding: 0;
    /* a star not given: barely there until the row is pointed at */
    color: var(--slate-600);
  }
  .large button {
    width: 30px;
    height: 32px;
    /* nothing reveals them on the player: the stars not given are plainly there */
    color: var(--slate-500);
  }
  button.lit,
  .large button.lit {
    color: var(--porcelain);
  }
  /* pointing at fewer stars than there are: the ones that would go */
  button.dim {
    color: var(--mist-dim);
  }
  button:focus-visible {
    outline: 2px solid var(--porcelain);
    outline-offset: -2px;
    border-radius: var(--r-sm);
  }
  :global(html[data-look="glass"]) button:not(.lit):not(.dim) {
    color: rgba(255, 255, 255, 0.16);
  }
  :global(html[data-look="glass"]) .large button:not(.lit):not(.dim) {
    color: rgba(255, 255, 255, 0.28);
  }
</style>
