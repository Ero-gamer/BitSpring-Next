<script lang="ts">
  // The library's folders: which are scanned, and which folders inside them are skipped.
  // Shown in the panel behind the album count in the library's toolbar.
  import { api, errorText, isAndroid, isIOS, type LibraryStatus, type LibrarySubfolder } from "./api";
  import { list, t, tn } from "./i18n.svelte";
  import { folderName, folderParts, isInside, parentOf, volumeLabel, volumeOf } from "./folderPlace";

  let {
    status = $bindable(),
    onadd,
    onimport,
  }: {
    status: LibraryStatus;
    /** Add a folder the platform's way (the system's folder picker). */
    onadd: () => Promise<void>;
    /** iPhone: copy files into the app's folder. */
    onimport: () => Promise<void>;
  } = $props();

  let error = $state("");

  /** What each library folder holds, by path (asked again whenever the library changes). */
  let held = $state<Record<string, LibrarySubfolder>>({});
  $effect(() => {
    void [status.revision, status.folders.length, status.skipped.length];
    api.librarySubfolders(null).then((roots) => (held = Object.fromEntries(roots.map((f) => [f.path, f]))));
  });

  interface Row {
    path: string;
    name: string;
    /** Under the name: how many tracks, and where the folder is. */
    facts: string;
    /** A phone's own folder: it can be skipped, not removed. */
    fixed: boolean;
    /** Skipped as a whole. */
    off: boolean;
    /** The folders inside it that are skipped. */
    except: string[];
    canSkipInside: boolean;
  }

  /** The library folder a skipped one belongs to: the nearest one that holds it. */
  const homeOf = (skipped: string) =>
    status.folders.filter((f) => f !== skipped && isInside(skipped, f)).sort((a, b) => b.length - a.length)[0];

  /** The folders by where they are: on a phone under their storage, elsewhere as one list. */
  const groups = $derived.by(() => {
    const out: { key: string; label: string | null; rows: Row[] }[] = [];
    const cards = new Set(status.folders.map((f) => (isAndroid ? volumeOf(f) : null)).filter((v) => v && !v.internal).map((v) => v!.key));
    for (const path of status.folders) {
      const vol = isAndroid ? volumeOf(path) : null;
      const key = vol?.key ?? "";
      let group = out.find((g) => g.key === key);
      if (!group) {
        // Two cards (or a card and a USB drive) are told apart by their ids.
        const label = vol ? volumeLabel(vol) + (!vol.internal && cards.size > 1 ? ` ${vol.key}` : "") : null;
        out.push((group = { key, label, rows: [] }));
      }
      const fixed = status.fixed.includes(path);
      const own = fixed && (isIOS || path.includes("/Android/data/"));
      const off = status.skipped.includes(path);
      const info = held[path];
      const where = vol ? vol.inside.slice(0, -1).join(" / ") : isIOS ? "" : parentOf(path);
      group.rows.push({
        path,
        name: own ? t("lib.ownFolder") : folderName(path),
        facts: list([
          // An empty folder of the app's own says what it is for, not "0 tracks".
          off ? t("lib.skippedState") : info && !(own && info.tracks === 0) && tn("lib.nTracks", info.tracks),
          own ? isAndroid && t("lib.ownFolderHelp", { path: vol?.inside.join("/") ?? path }) : where,
        ]),
        fixed,
        off,
        except: status.skipped.filter((s) => homeOf(s) === path),
        canSkipInside: !!info?.hasSubfolders,
      });
    }
    return out;
  });

  /** A skipped folder as its row's chip names it: the way down from the row's folder. */
  const below = (skipped: string, folder: string) => folderParts(skipped.slice(folder.length)).join(" / ");

  async function run(change: () => Promise<LibraryStatus | null | void>) {
    error = "";
    try {
      const s = await change();
      if (s) status = s;
    } catch (e) {
      error = errorText(e);
    }
  }

  const add = () => run(() => (isAndroid ? api.libraryPickFolder() : onadd()));

  // ---- "Skip a folder": walk the folders that hold a library folder's tracks ----------------
  /** The folders walked into, the library folder first. Empty: the list is closed. */
  let trail = $state<{ path: string; name: string }[]>([]);
  let choices = $state<LibrarySubfolder[]>([]);

  async function enter(f: { path: string; name: string }) {
    error = "";
    choices = await api.librarySubfolders(f.path);
    trail = [...trail, f];
  }

  async function back() {
    const up = trail.slice(0, -1);
    if (up.length) choices = await api.librarySubfolders(up.at(-1)!.path);
    trail = up;
  }

  async function skip(f: LibrarySubfolder) {
    trail = [];
    await run(() => api.librarySkipFolder(f.path));
  }
</script>

{#if trail.length}
  <div class="head">
    <button class="back" onclick={back} aria-label={t("lib.back")}>
      <svg width="8" height="12" viewBox="0 0 8 12" aria-hidden="true"><path d="M7 1L2 6l5 5" fill="none" stroke="currentColor" stroke-width="1.6" /></svg>
    </button>
    <p class="title">{trail.at(-1)!.name}</p>
  </div>
  <p class="note">{t("lib.skipHelp")}</p>
  {#if choices.length}
    <ul class="pick">
      {#each choices as f (f.path)}
        <li>
          <button class="into" disabled={!f.hasSubfolders} onclick={() => enter(f)} aria-label={f.hasSubfolders ? t("lib.openFolder", { name: f.name }) : undefined}>
            <span class="text">
              <span class="name">{f.name}</span>
              <span class="facts">{tn("lib.nTracks", f.tracks)}</span>
            </span>
            {#if f.hasSubfolders}
              <svg class="chev" width="8" height="12" viewBox="0 0 8 12" aria-hidden="true"><path d="M1 1l5 5-5 5" fill="none" stroke="currentColor" stroke-width="1.6" /></svg>
            {/if}
          </button>
          <button class="outline" onclick={() => skip(f)} aria-label={t("lib.skipThis", { path: f.name })}>{t("lib.skip")}</button>
        </li>
      {/each}
    </ul>
  {:else}
    <p class="note">{t("lib.skipNone")}</p>
  {/if}
{:else}
  <p class="title">{t(isIOS ? "lib.iosTitle" : isAndroid ? "lib.androidTitle" : "lib.foldersTitle")}</p>
  {#if isIOS}
    <p class="note">{t("lib.iosHelp")}</p>
    <p class="note">{t("lib.iosOnlyFolder")}</p>
  {/if}
  {#each groups as g (g.key)}
    {#if g.label}
      <p class="volume">{g.label}</p>
    {/if}
    <ul>
      {#each g.rows as r (r.path)}
        <li class="folder" class:off={r.off}>
          <div class="line">
            <span class="text" title={r.path}>
              <span class="name">{r.name}</span>
              {#if r.facts}
                <span class="facts">{r.facts}</span>
              {/if}
            </span>
            {#if r.off}
              <button class="act" onclick={() => run(() => api.libraryUnskipFolder(r.path))} aria-label={t("lib.includeFolder", { path: r.name })}>{t("lib.include")}</button>
            {:else if r.fixed}
              {#if !isIOS}
                <button class="act" onclick={() => run(() => api.librarySkipFolder(r.path))} aria-label={t("lib.skipThis", { path: r.name })}>{t("lib.skip")}</button>
              {/if}
            {:else}
              <button class="act danger" onclick={() => run(() => api.libraryRemoveFolder(r.path))} aria-label={t("lib.removeFolder", { path: r.path })}>{t("lib.remove")}</button>
            {/if}
          </div>
          <!-- What is left out of this folder reads on from its name: "Music, except Recordings". -->
          {#if !r.off && r.except.length}
            <div class="except">
              <span class="word">{t("lib.except")}</span>
              {#each r.except as s (s)}
                <button class="chip" onclick={() => run(() => api.libraryUnskipFolder(s))} aria-label={t("lib.includeFolder", { path: below(s, r.path) })} title={t("lib.includeFolder", { path: below(s, r.path) })}>
                  <span>{below(s, r.path)}</span>
                  <svg width="9" height="9" viewBox="0 0 9 9" aria-hidden="true"><path d="M1 1l7 7M8 1L1 8" stroke="currentColor" stroke-width="1.5" /></svg>
                </button>
              {/each}
              {#if r.canSkipInside}
                <button class="chip more" onclick={() => enter(r)} aria-label={t("lib.skipFolder")} title={t("lib.skipFolder")}>
                  <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><path d="M5 1v8M1 5h8" stroke="currentColor" stroke-width="1.5" /></svg>
                </button>
              {/if}
            </div>
          {:else if !r.off && r.canSkipInside}
            <div class="except">
              <button class="link" onclick={() => enter(r)}>{t("lib.skipFolder")}</button>
            </div>
          {/if}
        </li>
      {/each}
    </ul>
  {/each}
  {#if !status.folders.length}
    <p class="note">{t("lib.noFolders")}</p>
  {/if}
  <div class="actions">
    {#if isIOS}
      <button class="primary" onclick={() => run(onimport)}>{t("common.addFiles")}</button>
    {:else}
      <button class="primary" onclick={add}>{t("lib.addFolder")}</button>
    {/if}
    <button class="quiet" onclick={() => run(api.libraryRescan)} disabled={!!status.scanning}>{t("common.rescan")}</button>
  </div>
  {#if error}
    <p class="note error" role="alert">{error}</p>
  {/if}
  {#if status.failed}
    <p class="note failed">{tn("lib.failed", status.failed)}</p>
  {/if}
{/if}

<style>
  .title {
    margin: 0 0 8px;
    font-weight: 600;
  }
  .note {
    margin: 0 0 8px;
    font-size: var(--t-sm);
    color: var(--mist);
  }
  .note.error,
  .note.failed {
    margin-top: 10px;
  }
  .note.error {
    color: var(--alarm);
  }
  /* Which storage the folders below are on. */
  .volume {
    margin: 12px 0 0;
    font-size: var(--t-xs);
    color: var(--mist);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  li {
    border-bottom: 1px solid var(--slate-600);
  }
  .line,
  .pick li {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 44px;
  }
  /* One row shape everywhere: the folder's name, and under it what it holds and where it is.
     The name gives way first: it shortens with an ellipsis, the action beside it keeps its width. */
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    padding: 6px 0;
    text-align: start;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .facts {
    font-size: var(--t-xs);
    color: var(--mist);
    overflow-wrap: anywhere;
  }
  .off .name {
    color: var(--mist);
  }
  .act {
    flex: none;
    font-size: var(--t-sm);
    color: var(--mist);
    padding: 8px 2px;
  }
  .act:hover {
    color: var(--porcelain);
  }
  .act.danger:hover {
    color: var(--alarm);
  }

  .except {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    padding: 0 0 10px;
    font-size: var(--t-sm);
  }
  .word {
    color: var(--mist);
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    max-width: 100%;
    min-height: 28px;
    padding: 2px 10px;
    border: 1px solid var(--slate-500);
    border-radius: 999px;
  }
  .chip span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chip svg {
    flex: none;
    color: var(--mist);
  }
  .chip:hover {
    border-color: var(--mist);
  }
  .chip:hover svg {
    color: var(--porcelain);
  }
  .chip.more {
    padding: 2px 9px;
  }
  .link {
    color: var(--mist);
    padding: 2px 0;
    text-decoration: underline;
    text-decoration-color: var(--slate-500);
    text-underline-offset: 3px;
  }
  .link:hover {
    color: var(--porcelain);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px 14px;
    align-items: center;
    margin-top: 14px;
    font-size: var(--t-sm);
  }
  .primary,
  .outline {
    border-radius: var(--r-md);
    font-weight: 600;
  }
  .primary {
    padding: 8px 16px;
    background: var(--porcelain);
    color: var(--ink);
  }
  .primary:hover {
    filter: brightness(1.08);
  }
  .outline {
    flex: none;
    font-size: var(--t-sm);
    padding: 6px 12px;
    border: 1px solid var(--slate-500);
  }
  .outline:hover {
    border-color: var(--mist);
  }
  .quiet {
    color: var(--mist);
    padding: 8px 2px;
  }
  .quiet:hover:not(:disabled) {
    color: var(--porcelain);
  }
  button:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 4px;
    margin: -4px 0 6px -8px;
  }
  .head .title {
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .back {
    flex: none;
    width: 36px;
    height: 36px;
    display: grid;
    place-items: center;
    color: var(--mist);
    border-radius: var(--r-md);
  }
  .back:hover {
    color: var(--porcelain);
  }
  .pick li {
    gap: 8px;
  }
  .into {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 10px;
    align-self: stretch;
  }
  /* A folder with nothing deeper is still a full-strength row: only its Skip acts. */
  .into:disabled {
    opacity: 1;
  }
  .chev {
    flex: none;
    color: var(--mist);
  }

  /* A finger needs taller rows and wider targets than a pointer. */
  @media (max-width: 700px), (max-height: 500px), (pointer: coarse) {
    .line,
    .pick li {
      min-height: 52px;
    }
    .act,
    .quiet {
      padding: 12px 6px;
    }
    .primary {
      padding: 11px 18px;
    }
    .outline {
      padding: 10px 14px;
    }
    .chip {
      min-height: 36px;
      padding: 4px 12px;
    }
    .link {
      padding: 8px 0;
    }
    .except {
      gap: 8px;
    }
    .back {
      width: 44px;
      height: 44px;
    }
  }
</style>
