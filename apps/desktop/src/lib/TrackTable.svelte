<script lang="ts" module>
  export type SortKey = "number" | "title" | "artist" | "album" | "year" | "duration" | "format" | "dr" | "rating" | "fav";
</script>

<script lang="ts">
  import type { Snippet } from "svelte";
  import { clock, khz, resolution, type Dynamics, type LibTrack } from "./api";
  import { collator as makeCollator, t } from "./i18n.svelte";
  import Heart from "./Heart.svelte";
  import Stars from "./Stars.svelte";
  import { markable, marks, setFavorite, setRating } from "./marks.svelte";

  let {
    tracks,
    currentPath = null,
    columns = ["number", "title", "artist", "album", "year", "duration", "format"],
    sortable = true,
    sortKey = $bindable<SortKey>("artist"),
    sortAsc = $bindable(true),
    shown = $bindable<LibTrack[]>([]),
    onplay,
    actions,
    marker,
    dimmed,
    dynamics = {},
  }: {
    tracks: LibTrack[];
    currentPath?: string | null;
    columns?: SortKey[];
    sortable?: boolean;
    sortKey?: SortKey;
    sortAsc?: boolean;
    /** The tracks in the order the table shows them (for the caller to read). */
    shown?: LibTrack[];
    /** Play `list` starting at `index` (the list as currently shown). */
    onplay: (list: LibTrack[], index: number) => void;
    /** Buttons at the end of each row (index = position in `tracks` as given). */
    actions?: Snippet<[LibTrack, number]>;
    /** Something small after each title (the quality check's badge). */
    marker?: Snippet<[LibTrack]>;
    /** Rows that can't be played (missing files): shown greyed, not playable. */
    dimmed?: (t: LibTrack) => boolean;
    /** What was measured of the tracks' dynamics, by path (for the "dr" column). */
    dynamics?: Record<string, Dynamics>;
  } = $props();

  const drOf = (t: LibTrack) => dynamics[t.path]?.dr ?? null;

  // The listener's marks close every list of files: the stars (when switched on in Settings)
  // and the heart, next to the row's own buttons. A list of streaming tracks has neither.
  let shownColumns = $derived<SortKey[]>(
    tracks.some((tr) => markable(tr.path)) ? [...columns, ...(marks.ratings ? (["rating", "fav"] as const) : (["fav"] as const))] : columns,
  );

  const collator = $derived(makeCollator());
  const artistOf = (t: LibTrack) => t.albumArtist ?? t.artist ?? "";
  /** Empty values (untagged files) sort last, whichever direction. */
  const filledFirst = (a: string, b: string) => Number(!a) - Number(!b);
  // Within an artist or album, keep album order (disc, track) as the tie-breaker.
  const albumOrder = (a: LibTrack, b: LibTrack) =>
    collator.compare(a.album ?? "", b.album ?? "") ||
    (a.discNumber ?? 1) - (b.discNumber ?? 1) ||
    (a.trackNumber ?? 999) - (b.trackNumber ?? 999) ||
    collator.compare(a.path, b.path);

  let sorted = $derived.by(() => {
    if (!sortable) return tracks;
    const dir = sortAsc ? 1 : -1;
    const by: Record<SortKey, (a: LibTrack, b: LibTrack) => number> = {
      number: (a, b) => (a.trackNumber ?? 999) - (b.trackNumber ?? 999),
      title: (a, b) => collator.compare(a.title, b.title),
      artist: (a, b) => collator.compare(artistOf(a), artistOf(b)) || collator.compare(a.year ?? "", b.year ?? ""),

      album: (a, b) => collator.compare(a.album ?? "", b.album ?? ""),
      year: (a, b) => collator.compare(a.year ?? "", b.year ?? ""),
      duration: (a, b) => a.duration - b.duration,
      format: (a, b) => (a.bits ?? 0) - (b.bits ?? 0) || a.sampleRate - b.sampleRate,
      dr: (a, b) => (drOf(a) ?? 0) - (drOf(b) ?? 0),
      rating: (a, b) => marks.rating(a.path) - marks.rating(b.path),
      // (Favourites together, in the order of the list they came from.)
      fav: () => 0,
    };
    const value: Partial<Record<SortKey, (t: LibTrack) => string>> = {
      artist: artistOf,
      album: (t) => t.album ?? "",
      year: (t) => t.year ?? "",
      // (Not measured yet: last, like an empty tag.)
      dr: (t) => (drOf(t) == null ? "" : "x"),
      // (No stars, no heart: after the ones that have them.)
      rating: (t) => (marks.rating(t.path) ? "x" : ""),
      fav: (t) => (marks.fav(t.path) ? "x" : ""),
    };
    const v = value[sortKey];
    return [...tracks].sort(
      (a, b) => (v ? filledFirst(v(a), v(b)) : 0) || dir * by[sortKey](a, b) || albumOrder(a, b),
    );
  });

  $effect(() => {
    shown = sorted;
  });

  /** A touch screen has no double-click: one tap on a row plays it. */
  const touch = typeof window !== "undefined" && window.matchMedia("(hover: none)").matches;

  function sortBy(k: SortKey) {
    if (!sortable) return;
    if (sortKey === k) sortAsc = !sortAsc;
    else {
      sortKey = k;
      // Numbers start from the top: longest, best, most stars.
      sortAsc = !["duration", "format", "rating"].includes(k);
    }
  }
</script>

<table class="tracks">
  <thead>
    <tr>
      {#each shownColumns as c (c)}
        <th class={c} title={c === "dr" ? t("dyn.help") : undefined} aria-label={c === "fav" ? t("col.fav") : undefined} aria-sort={sortable && sortKey === c ? (sortAsc ? "ascending" : "descending") : undefined}>
          {#if c === "fav"}
            <!-- the heart stands for its name; sorted by it, the favourites come first. A list
                 that can't be sorted has no heart up here: it would look like a button that
                 does nothing (the rows' own hearts say what the column is). -->
            {#if sortable}
              <button onclick={() => ((sortKey = c), (sortAsc = true))} aria-label={t("col.fav")} title={t("col.fav")}><Heart size={13} on={sortKey === c} /></button>
            {:else}
              <span class="th"></span>
            {/if}
          {:else if sortable}
            <button onclick={() => sortBy(c)}>
              {t(`col.${c}`)}{#if sortKey === c}<span class="arrow" aria-hidden="true">{sortAsc ? "▲" : "▼"}</span>{/if}
            </button>
          {:else}
            <span class="th">{t(`col.${c}`)}</span>
          {/if}
        </th>
      {/each}
      {#if actions}<th class="actions"><span class="th"></span></th>{/if}
    </tr>
  </thead>
  <tbody>
    {#each sorted as tr, i (tr.path + i)}
      {@const off = dimmed?.(tr) ?? false}
      <tr class:current={tr.path === currentPath} class:off ondblclick={() => !off && onplay(sorted, i)} onclick={() => touch && !off && onplay(sorted, i)}>
        {#each shownColumns as c (c)}
          <td class={c}>
            {#if c === "number"}
              {#if tr.path === currentPath}
                <svg width="10" height="10" viewBox="0 0 10 10" aria-label={t("common.playing")}><path d="M2 1v8l7-4z" fill="currentColor" /></svg>
              {:else}
                <button class="play-row" disabled={off} onclick={(e) => { e.stopPropagation(); onplay(sorted, i); }} aria-label={t("track.play", { title: tr.title })}>
                  <span class="n num">{tr.trackNumber ?? "–"}</span>
                  <svg class="p" width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><path d="M2 1v8l7-4z" fill="currentColor" /></svg>
                </button>
              {/if}
            {:else if c === "title"}
              {#if marker}
                <span class="title-cell"><span class="title-text">{tr.title}</span>{@render marker(tr)}</span>
              {:else}
                {tr.title}
              {/if}
            {:else if c === "artist"}
              {tr.artist ?? "—"}
            {:else if c === "album"}
              {tr.album ?? "—"}
            {:else if c === "year"}
              <span class="num">{tr.year ?? ""}</span>
            {:else if c === "duration"}
              <span class="num">{clock(tr.duration)}</span>
            {:else if c === "dr"}
              {@const dr = drOf(tr)}
              <span class="num" class:none={dr == null}>{dr == null ? "–" : Math.round(dr)}</span>
            {:else if c === "fav"}
              {#if markable(tr.path)}
                {@const on = marks.fav(tr.path)}
                <button
                  class="heart"
                  class:on
                  aria-pressed={on}
                  aria-label={t(on ? "fav.removeNamed" : "fav.addNamed", { title: tr.title })}
                  title={t(on ? "fav.remove" : "fav.add")}
                  onclick={(e) => (e.stopPropagation(), setFavorite(tr.path, !on))}
                  ondblclick={(e) => e.stopPropagation()}
                >
                  <Heart {on} size={15} beats />
                </button>
              {/if}
            {:else if c === "rating"}
              {#if markable(tr.path)}
                <Stars value={marks.rating(tr.path)} label={t("rate.of", { title: tr.title })} onchange={(n) => setRating(tr.path, n)} />
              {/if}
            {:else if c === "format"}
              <span class="fmt" class:hi={tr.hiRes} class:lossy={!tr.lossless}>
                {tr.format}{tr.lossless && tr.sampleRate ? (tr.bits ? ` ${resolution(tr.bits, tr.sampleRate)}` : ` ${khz(tr.sampleRate)} ${t("unit.khz")}`) : ""}
              </span>
            {/if}
          </td>
        {/each}
        {#if actions}<td class="actions">{@render actions(tr, sortable ? tracks.indexOf(tr) : i)}</td>{/if}
      </tr>
    {/each}
  </tbody>
</table>

<style>
  .tracks {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--t-sm);
    table-layout: fixed;
  }
  th {
    position: sticky;
    top: 0;
    z-index: 1;
    background: var(--slate-800);
    text-align: start;
    font-weight: 500;
    color: var(--mist);
    padding: 0;
    border-bottom: 1px solid var(--slate-600);
  }
  th button,
  .th {
    display: block;
    width: 100%;
    text-align: inherit;
    padding: 8px 10px;
    color: inherit;
  }
  th button:hover {
    color: var(--porcelain);
  }
  th[aria-sort] {
    color: var(--porcelain);
  }
  .arrow {
    font-size: 8px;
    margin-inline-start: 5px;
    vertical-align: 1px;
  }
  td {
    padding: 7px 10px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    border-bottom: 1px solid rgba(43, 65, 76, 0.5);
  }
  tbody tr {
    /* long libraries: let the browser skip layout for rows far off-screen */
    content-visibility: auto;
    contain-intrinsic-size: auto 33px;
  }
  /* hover only where there is a pointer: on a touch screen it sticks after a tap */
  @media (hover: hover) {
    tbody tr:hover {
      background: var(--slate-700);
    }
    tr:hover .play-row .n {
      opacity: 0;
    }
    tr:hover .play-row .p {
      opacity: 1;
    }
  }
  tr.current td {
    color: var(--live);
  }
  tr.off td {
    color: var(--mist-dim);
  }
  .actions {
    width: 76px;
    text-align: right;
    overflow: visible;
  }
  .number {
    width: 44px;
    text-align: end;
  }
  th.number button,
  th.number .th {
    text-align: end;
  }
  .year {
    width: 60px;
  }
  .duration {
    width: 64px;
    text-align: end;
  }
  th.duration button,
  th.duration .th {
    text-align: end;
  }
  .format {
    width: 120px;
  }
  .dr {
    width: 52px;
    text-align: end;
  }
  th.dr button,
  th.dr .th {
    text-align: end;
  }
  td.dr {
    color: var(--mist);
  }
  .none {
    color: var(--mist-dim);
  }
  /* --- the listener's marks: stars, then the heart, beside the row's own buttons --- */
  .rating {
    width: 106px;
  }
  td.rating {
    padding-block: 0;
  }
  .fav {
    width: 34px;
    text-align: center;
  }
  th.fav button,
  th.fav .th {
    display: grid;
    place-items: center;
    padding-inline: 0;
    /* as tall as the headers made of words */
    min-height: 37px;
  }
  td.fav {
    padding: 0;
    overflow: visible;
  }
  .heart {
    display: grid;
    place-items: center;
    width: 34px;
    height: 32px;
    /* not a favourite: the outline is there, faint — a button nobody sees isn't pressed */
    color: var(--slate-500);
  }
  .heart.on {
    color: var(--porcelain);
  }
  .heart:focus-visible {
    color: var(--mist);
    outline: 2px solid var(--porcelain);
    outline-offset: -4px;
    border-radius: var(--r-md);
  }
  .heart.on:focus-visible {
    color: var(--porcelain);
  }
  @media (hover: hover) {
    tr:hover .heart:not(.on) {
      color: var(--mist);
    }
    tr:hover .heart:not(.on):hover {
      color: var(--porcelain);
    }
    /* the stars nobody gave show with the row, like the heart's outline */
    tbody tr:not(:hover) td.rating :global(.stars:not(.rated):not(:focus-within)) {
      visibility: hidden;
    }
  }
  /* the playing row is in the signal's colour; its marks stay the listener's own */
  tr.current td .heart.on {
    color: var(--porcelain);
  }
  td.title {
    font-weight: 500;
  }
  .title-cell {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .title-text {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  td.artist,
  td.album,
  td.year {
    color: var(--mist);
  }
  .play-row {
    display: inline-grid;
    place-items: center end;
    width: 100%;
  }
  .play-row > * {
    grid-area: 1 / 1;
  }
  .play-row .p {
    opacity: 0;
    color: var(--porcelain);
  }
  .n {
    color: var(--mist-dim);
  }
  .play-row:focus-visible .p {
    opacity: 1;
  }
  .fmt {
    color: var(--mist);
  }
  .fmt.hi {
    color: var(--porcelain);
  }
  .fmt.lossy {
    color: var(--mist-dim);
  }

  /* phone: number, title, length — the rest is on the album page */
  @media (max-width: 700px), (max-height: 500px) {
    .artist,
    .album,
    .year,
    .dr,
    .rating,
    .format {
      display: none;
    }
    td {
      padding-block: 11px;
    }
    td.fav {
      padding-block: 0;
    }
    .heart {
      height: 42px;
    }
  }
</style>
