<script lang="ts">
  import { t } from "./i18n.svelte";
  import { newAddress, remote } from "./remote.svelte";

  // In the header only while the remote is on: the code at hand for another phone, and a
  // standing sign that the player can be controlled from the network.
  let open = $state(false);
  let root = $state<HTMLElement>();

  function onWindowClick(e: MouseEvent) {
    if (open && root && !e.composedPath().includes(root)) open = false;
  }
</script>

<svelte:window onclick={onWindowClick} onkeydown={(e) => e.key === "Escape" && (open = false)} />

{#if remote.status?.enabled && remote.status.url}
  <div class="remote" bind:this={root}>
    <button class="trigger" onclick={() => (open = !open)} aria-expanded={open} aria-haspopup="dialog" aria-label={t("remote.setting")} title={t("remote.setting")}>
      <!-- a phone -->
      <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">
        <rect x="4.2" y="1.4" width="7.6" height="13.2" rx="1.7" fill="none" stroke="currentColor" stroke-width="1.3" />
        <path d="M7 12.3h2" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
      </svg>
    </button>

    {#if open}
      <div class="panel" role="dialog" aria-label={t("remote.setting")}>
        <p class="title">{t("remote.setting")}</p>
        <p class="note">{t("remote.scan")}</p>
        <!-- Our own SVG, made by the app from the address. -->
        <div class="qr" aria-hidden="true">{@html remote.status.qr}</div>
        <p class="url">{remote.status.url}</p>
        <button class="quiet" onclick={newAddress}>{t("remote.newAddress")}</button>
        <p class="note">{t("remote.newHelp")}</p>
      </div>
    {/if}
  </div>
{/if}

<style>
  .remote {
    position: relative;
    flex: none;
  }
  .trigger {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: var(--r-md);
    border: 1px solid var(--slate-600);
    color: var(--porcelain);
  }
  .trigger:hover,
  .trigger[aria-expanded="true"] {
    border-color: var(--slate-500);
  }
  .panel {
    position: absolute;
    inset-inline-end: 0;
    top: calc(100% + 8px);
    width: 252px;
    z-index: 20;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    background: var(--slate-700);
    border: 1px solid var(--slate-500);
    border-radius: var(--r-lg);
    padding: 14px 16px 16px;
    box-shadow: 0 18px 40px rgba(5, 12, 15, 0.55);
    max-height: calc(100vh - 72px);
    overflow-y: auto;
  }
  p {
    margin: 0;
  }
  .title {
    font-weight: 600;
  }
  .note {
    margin-top: 4px;
    font-size: var(--t-sm);
    line-height: 1.5;
    color: var(--mist);
  }
  .qr {
    align-self: stretch;
    margin-top: 12px;
    padding: 10px;
    border-radius: var(--r-md);
    background: #fff;
    line-height: 0;
  }
  .qr :global(svg) {
    width: 100%;
    height: auto;
  }
  .url {
    margin-top: 10px;
    font-size: var(--t-xs);
    color: var(--porcelain);
    user-select: all;
    -webkit-user-select: all;
    word-break: break-all;
  }
  .quiet {
    margin-top: 12px;
    padding: 6px 12px;
    border-radius: var(--r-md);
    border: 1px solid var(--slate-500);
    font-size: var(--t-sm);
  }
  .quiet:hover {
    border-color: var(--mist);
  }
</style>
