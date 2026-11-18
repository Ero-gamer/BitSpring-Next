<script lang="ts">
  // The library's folders: which are scanned, which are skipped. Shown in the panel behind
  // the album count in the library's toolbar.
  import { api, errorText, isAndroid, isIOS, type LibraryStatus, type LibrarySubfolder } from "./api";
  import { list, sentences, t, tn } from "./i18n.svelte";

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
  /** "Skip a folder": the folders walked into so far (empty: the library's own folders). */
  let trail = $state<LibrarySubfolder[]>([]);
  let choices = $state<LibrarySubfolder[] | null>(null);
  /** Opened straight inside the library's only folder: there is no level above it to go back to. */
  let single = $state(false);

  /** A folder on exactly one list: a built-in one that is skipped shows under "Skipped" only. */
  const scanned = $derived(status.folders.filter((f) => !status.skipped.includes(f)));

  const parts = (path: string) => path.split(/[\\/]+/).filter(Boolean);
  const nameOf = (path: string) => parts(path).at(-1) ?? path;

  /** Where a folder is, in the user's words: on a phone "Internal storage / Music" rather
   *  than /storage/emulated/0/Music. */
  function placeOf(path: string): string {
    const sep = path.includes("\\") ? "\\" : "/";
    const cut = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
    const parent = cut > 0 ? path.slice(0, cut) : sep;
    if (isAndroid) {
      const m = /^\/storage\/(emulated\/\d+|[^/]+)(\/.*)?$/.exec(parent);
      if (m) return [t(m[1].startsWith("emulated/") ? "lib.internalStorage" : "lib.removableStorage"), ...parts(m[2] ?? "")].join(" / ");
    }
    if (isIOS) {
      // Inside the app's own folder: only what is below it means anything to the user.
      const own = status.fixed[0];
      if (own && parent.startsWith(own)) return ["BitSpring", ...parts(parent.slice(own.length))].join(" / ");
    }
    return parent;
  }

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

  async function show(folder: LibrarySubfolder | null) {
    choices = await api.librarySubfolders(folder?.path ?? null);
  }

  async function openChooser() {
    error = "";
    trail = [];
    single = false;
    const roots = await api.librarySubfolders(null);
    // One folder (a computer with one music folder, an iPhone): start inside it.
    if (roots.length === 1) {
      single = true;
      await enter(roots[0]);
    } else {
      choices = roots;
    }
  }

  async function enter(f: LibrarySubfolder) {
    await show(f);
    trail = [...trail, f];
  }

  async function back() {
    if (trail.length <= (single ? 1 : 0)) {
      choices = null;
      return;
    }
    const up = trail.slice(0, -1);
    await show(up.at(-1) ?? null);
    trail = up;
  }

  async function skip(f: LibrarySubfolder) {
    choices = null;
    await run(() => api.librarySkipFolder(f.path));
  }

  /** At the top of the list only a phone's built-in folders can be skipped: the rest are
   *  the user's own, and those are removed instead. */
  const skippable = (f: LibrarySubfolder) => trail.length > 0 || status.fixed.includes(f.path);
</script>

{#if choices}
  <div class="head">
    <button class="back" onclick={back} aria-label={t("lib.back")}>
      <svg width="8" height="12" viewBox="0 0 8 12" aria-hidden="true"><path d="M7 1L2 6l5 5" fill="none" stroke="currentColor" stroke-width="1.6" /></svg>
    </button>
    <p class="title">{trail.length > (single ? 1 : 0) ? trail.at(-1)?.name : t("lib.skipChoose")}</p>
  </div>
  {#if choices.length}
    <ul class="pick">
      {#each choices as f (f.path)}
        <li>
          <button class="into" disabled={!f.hasSubfolders} onclick={() => enter(f)} aria-label={f.hasSubfolders ? t("lib.openFolder", { name: f.name }) : undefined}>
            <span class="text">
              <span class="name">{f.name}</span>
              <!-- Two of a phone's own folders are both called Music: at the top, say where each is. -->
              <span class="place">{trail.length ? tn("lib.nTracks", f.tracks) : list([tn("lib.nTracks", f.tracks), placeOf(f.path)])}</span>
            </span>
            {#if f.hasSubfolders}
              <svg class="chev" width="8" height="12" viewBox="0 0 8 12" aria-hidden="true"><path d="M1 1l5 5-5 5" fill="none" stroke="currentColor" stroke-width="1.6" /></svg>
            {/if}
          </button>
          {#if skippable(f)}
            <button class="chip" onclick={() => skip(f)} aria-label={t("lib.skipThis", { path: f.name })}>{t("lib.skip")}</button>
          {/if}
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
  {:else}
    {#if isAndroid}
      <p class="note">{sentences([t("lib.androidHelp"), t("lib.androidAdd")])}</p>
    {/if}
    {#if scanned.length}
      <ul>
        {#each scanned as f (f)}
          <li>
            <span class="text" title={f}>
              <span class="name">{nameOf(f)}</span>
              <span class="place">{placeOf(f)}</span>
            </span>
            {#if status.fixed.includes(f)}
              <button class="act" onclick={() => run(() => api.librarySkipFolder(f))} aria-label={t("lib.skipThis", { path: f })}>{t("lib.skip")}</button>
            {:else}
              <button class="act danger" onclick={() => run(() => api.libraryRemoveFolder(f))} aria-label={t("lib.removeFolder", { path: f })}>{t("lib.remove")}</button>
            {/if}
          </li>
        {/each}
      </ul>
    {:else}
      <p class="note">{t("lib.noFolders")}</p>
    {/if}
  {/if}
  <div class="actions">
    {#if isIOS}
      <button class="primary" onclick={() => run(onimport)}>{t("common.addFiles")}</button>
    {:else}
      <button class="primary" onclick={add}>{t("lib.addFolder")}</button>
    {/if}
    <button class="quiet" onclick={() => run(api.libraryRescan)} disabled={!!status.scanning}>{t("common.rescan")}</button>
  </div>

  <p class="title second">{t("lib.skipTitle")}</p>
  {#if status.skipped.length}
    <ul>
      {#each status.skipped as f (f)}
        <li>
          <span class="text" title={f}>
            <span class="name">{nameOf(f)}</span>
            <span class="place">{placeOf(f)}</span>
          </span>
          <button class="act" onclick={() => run(() => api.libraryUnskipFolder(f))} aria-label={t("lib.includeFolder", { path: f })}>{t("lib.include")}</button>
        </li>
      {/each}
    </ul>
  {/if}
  <p class="note">{t("lib.skipHelp")}</p>
  <div class="actions">
    <button class="ghost" onclick={openChooser} disabled={status.tracks === 0}>{t("lib.skipFolder")}</button>
  </div>
  {#if error}
    <p class="note error" role="alert">{error}</p>
  {/if}
  {#if status.failed}
    <p class="note">{tn("lib.failed", status.failed)}</p>
  {/if}
{/if}

<style>
  .title {
    margin: 0 0 8px;
    font-weight: 600;
  }
  .title.second {
    margin-top: 18px;
  }
  .note {
    margin: 0 0 8px;
    font-size: var(--t-sm);
    color: var(--mist);
  }
  .note.error {
    margin-top: 10px;
    color: var(--alarm);
  }
  ul {
    list-style: none;
    margin: 0 0 4px;
    padding: 0;
  }
  li {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 44px;
    border-bottom: 1px solid var(--slate-600);
  }
  /* One row shape everywhere: the folder's name, and under it where it is (or what it holds). */
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    padding: 5px 0;
    text-align: start;
  }
  .name,
  .place {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .place {
    font-size: var(--t-xs);
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
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px 14px;
    align-items: center;
    margin-top: 12px;
    font-size: var(--t-sm);
  }
  .primary,
  .ghost {
    padding: 8px 16px;
    border-radius: var(--r-md);
    font-weight: 600;
  }
  .primary {
    background: var(--porcelain);
    color: var(--ink);
  }
  .primary:hover {
    filter: brightness(1.08);
  }
  .ghost {
    border: 1px solid var(--slate-500);
  }
  .ghost:hover:not(:disabled) {
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

  /* "Skip a folder": walk the folders that hold the library's tracks. */
  .head {
    display: flex;
    align-items: center;
    gap: 4px;
    margin: -4px 0 4px -8px;
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
  .chip {
    flex: none;
    font-size: var(--t-sm);
    padding: 6px 12px;
    border: 1px solid var(--slate-500);
    border-radius: var(--r-md);
  }
  .chip:hover {
    border-color: var(--mist);
  }

  /* A finger needs taller rows and wider targets than a pointer. */
  @media (max-width: 700px), (max-height: 500px), (pointer: coarse) {
    li {
      min-height: 52px;
    }
    .act,
    .quiet {
      padding: 12px 6px;
    }
    .primary,
    .ghost {
      padding: 11px 18px;
    }
    .chip {
      padding: 10px 14px;
    }
    .back {
      width: 44px;
      height: 44px;
    }
  }
</style>
