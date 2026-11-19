<script lang="ts">
  import { onMount } from "svelte";
  import { api, clock, errorText, type AbxPrepared, type AbxResult, type AbxWhich } from "./api";
  import type { AbxRequest } from "./abx.svelte";
  import { num, t } from "./i18n.svelte";

  let { request, device, exclusive, onclose }: { request: AbxRequest; device: string; exclusive: boolean; onclose: () => void } = $props();

  const TRIALS = [8, 12, 16, 24];

  let prepared = $state<AbxPrepared | null>(null);
  let error = $state<string | null>(null);
  let total = $state(16);
  let done = $state(0);
  let result = $state<AbxResult | null>(null);
  let listening = $state<AbxWhich>("a");
  let position = $state(0);
  let duration = $state(0);
  let paused = $state(false);
  /** A switch or an answer is on its way to the player. */
  let busy = $state(false);

  let ready = $derived(prepared !== null && !prepared.identical);
  let khz = (hz: number) => num(hz / 1000, { maximumFractionDigits: 1 });

  /** What was done to make the two comparable, in the order it matters to a listener. */
  let notes = $derived.by(() => {
    const p = prepared;
    if (!p) return [];
    const out: string[] = [];
    if (request.kind === "processing") out.push(t("abx.n.processing"));
    if (p.a.resampled || p.b.resampled) {
      const side = p.a.resampled ? p.a : p.b;
      out.push(t("abx.n.rate", { rate: khz(p.rate), which: p.a.resampled ? "A" : "B", from: khz(side.rate) }));
    }
    if (request.kind === "processing") {
      out.push(p.a.gainDb < -0.01 ? t("abx.n.both", { db: num(-p.a.gainDb, { maximumFractionDigits: 1 }) }) : t("abx.n.matched"));
    } else {
      const down = p.a.gainDb < -0.01 ? (["A", p.a.gainDb] as const) : p.b.gainDb < -0.01 ? (["B", p.b.gainDb] as const) : null;
      out.push(down ? t("abx.n.gain", { which: down[0], db: num(-down[1], { maximumFractionDigits: 1 }) }) : t("abx.n.level"));
      if (Math.abs(p.offsetMs) >= 1) {
        out.push(t("abx.n.offset", { which: p.offsetMs > 0 ? "B" : "A", ms: num(Math.abs(p.offsetMs), { maximumFractionDigits: 0 }) }));
      }
    }
    if (p.shortened) out.push(t("abx.n.short", { time: clock(p.seconds) }));
    out.push(t("abx.n.fade"));
    return out;
  });

  async function prepare() {
    try {
      const info = await api.abxStart(request.kind, request.a, request.kind === "files" ? request.b : null, device, exclusive, total);
      prepared = info.prepared;
      total = info.trials;
      duration = info.prepared.seconds;
    } catch (e) {
      error = errorText(e);
    }
  }

  async function listen(which: AbxWhich) {
    if (!ready || busy) return;
    busy = true;
    listening = which;
    try {
      await api.abxListen(which);
      paused = false;
    } catch (e) {
      error = errorText(e);
    }
    busy = false;
  }

  async function answer(isA: boolean) {
    if (!ready || busy || result) return;
    busy = true;
    try {
      const p = await api.abxAnswer(isA);
      done = p.done;
      result = p.result;
    } catch (e) {
      error = errorText(e);
    }
    busy = false;
  }

  async function again() {
    result = null;
    done = 0;
    listening = "a";
    await api.abxAgain();
  }

  async function setTrials(n: number) {
    total = await api.abxSetTrials(n);
  }

  function togglePause() {
    paused = !paused;
    api.setPaused(paused);
  }

  function close() {
    api.abxStop();
    onclose();
  }

  onMount(() => {
    prepare();
    // The position, and the music starting over when it has run out.
    const timer = setInterval(async () => {
      if (!ready || error) return;
      const s = await api.status();
      if (s.active && !s.finished) {
        position = s.position;
        paused = s.paused;
      } else if (!busy && !paused) {
        api.abxListen(listening).catch(() => {});
      }
    }, 200);
    return () => clearInterval(timer);
  });

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") return close();
    if (!ready || e.metaKey || e.ctrlKey || e.altKey) return;
    const k = e.key.toLowerCase();
    // (By the key's place too, so A, B and X work in any keyboard layout.)
    if (k === "a" || e.code === "KeyA") listen("a");
    else if (k === "b" || e.code === "KeyB") listen("b");
    else if (k === "x" || e.code === "KeyX") listen("x");
    else if (e.key === "1") answer(true);
    else if (e.key === "2") answer(false);
    else if (e.code === "Space") {
      e.preventDefault();
      if (!e.repeat) togglePause();
    } else return;
    e.stopPropagation();
  }

  let percent = $derived(result ? num(result.p * 100, { maximumSignificantDigits: 2 }) : "");
</script>

<svelte:window onkeydown={onKey} />

<div class="bg" role="presentation">
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="abx-title">
    <header>
      <div>
        <p class="kind">{t("abx.title")}</p>
        <h3 id="abx-title">{request.title}</h3>
      </div>
      <button class="close" onclick={close} aria-label={t("q.close")}>
        <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><path d="M2 2l8 8M10 2l-8 8" stroke="currentColor" stroke-width="1.6" /></svg>
      </button>
    </header>

    <dl class="sides">
      <div><dt>A</dt><dd>{request.kind === "files" ? request.aName : t("abx.asIs")}</dd></div>
      <div><dt>B</dt><dd>{request.kind === "files" ? request.bName : t("abx.processed")}</dd></div>
    </dl>

    {#if error}
      <p class="state bad" role="alert">{t("abx.failed", { error })}</p>
    {:else if !prepared}
      <p class="state" role="status">{t("abx.preparing")}</p>
      <div class="wait" aria-hidden="true"></div>
    {:else if prepared.identical}
      <p class="state">{t("abx.identical")}</p>
    {:else}
      {#if prepared.likeness < 0.3}
        <p class="state bad">{t("abx.unlike")}</p>
      {/if}

      <!-- The test itself: three keys, of which X is the unknown. -->
      <div class="keys" role="group" aria-label={t("abx.listenTo")}>
        {#each ["a", "x", "b"] as const as w (w)}
          <button class="choice" class:on={listening === w} class:unknown={w === "x"} aria-pressed={listening === w} onclick={() => listen(w)} aria-label={t("abx.listen", { which: w.toUpperCase() })}>
            {w.toUpperCase()}
          </button>
        {/each}
      </div>

      <div class="transport">
        <button class="pause" onclick={togglePause} aria-label={t(paused ? "common.play" : "common.pause")}>
          {#if paused}
            <svg width="12" height="12" viewBox="0 0 10 10" aria-hidden="true"><path d="M2 1v8l7-4z" fill="currentColor" /></svg>
          {:else}
            <svg width="12" height="12" viewBox="0 0 10 10" aria-hidden="true"><path d="M2 1h2v8H2zM6 1h2v8H6z" fill="currentColor" /></svg>
          {/if}
        </button>
        <span class="time">{clock(position)}</span>
        <input
          type="range"
          min="0"
          max={duration}
          step="0.1"
          value={position}
          aria-label={t("remote.position")}
          onchange={(e) => api.seek(Number(e.currentTarget.value))}
          style={`--at: ${duration ? (position / duration) * 100 : 0}%`}
        />
        <span class="time">{clock(duration)}</span>
      </div>

      {#if result}
        <div class="result" role="status">
          <p class="score">{t("abx.result", { correct: num(result.correct), total: num(result.total) })}</p>
          <p class="verdict">{t(result.p < 0.05 ? "abx.heard" : "abx.notHeard")}</p>
          <p class="chance">{t("abx.chance", { percent })}</p>
          <ol class="marks" aria-label={t("abx.answers")}>
            {#each result.trials as tr, i (i)}
              <li class:wrong={tr.xIsA !== tr.saidA} title={t("abx.mark", { n: num(i + 1), x: tr.xIsA ? "A" : "B", said: tr.saidA ? "A" : "B" })}>{tr.xIsA ? "A" : "B"}</li>
            {/each}
          </ol>
          <button class="ghost" onclick={again}>{t("abx.again")}</button>
        </div>
      {:else}
        <div class="ask">
          <p class="trial">{t("abx.trial", { n: num(done + 1), total: num(total) })}</p>
          <div class="answers">
            <button class="primary" disabled={busy} onclick={() => answer(true)}>{t("abx.isA")}</button>
            <button class="primary" disabled={busy} onclick={() => answer(false)}>{t("abx.isB")}</button>
          </div>
          {#if done === 0}
            <div class="count">
              <span id="abx-trials">{t("abx.trials")}</span>
              <div class="seg" role="radiogroup" aria-labelledby="abx-trials">
                {#each TRIALS as n (n)}
                  <button role="radio" aria-checked={total === n} class:sel={total === n} onclick={() => setTrials(n)}>{num(n)}</button>
                {/each}
              </div>
            </div>
          {/if}
        </div>
        <p class="how">{t("abx.how")}</p>
      {/if}

      <ul class="notes">
        {#each notes as n (n)}<li>{n}</li>{/each}
      </ul>
    {/if}
  </div>
</div>

<style>
  .bg {
    position: fixed;
    inset: 0;
    z-index: 70;
    display: grid;
    place-items: center;
    padding: var(--safe-top, env(safe-area-inset-top)) 16px var(--safe-bottom, env(safe-area-inset-bottom));
    background: rgba(5, 12, 15, 0.72);
  }
  .dialog {
    width: min(520px, 100%);
    max-height: calc(100dvh - 32px);
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 18px 20px 18px;
    border-radius: var(--r-lg);
    background: var(--slate-800);
    border: 1px solid var(--slate-500);
    box-shadow: 0 24px 60px rgba(0, 0, 0, 0.45);
  }
  header {
    display: flex;
    justify-content: space-between;
    align-items: start;
    gap: 12px;
  }
  p {
    margin: 0;
  }
  .kind {
    font-size: var(--t-xs);
    color: var(--mist);
  }
  h3 {
    margin: 2px 0 0;
    font-family: var(--font-display);
    font-size: var(--t-lg);
    font-weight: 600;
  }
  .close {
    padding: 8px;
    border-radius: var(--r-sm);
    color: var(--mist);
  }
  .close:hover {
    color: var(--porcelain);
    background: var(--slate-700);
  }

  .sides {
    margin: 0;
    display: grid;
    gap: 4px;
    font-size: var(--t-sm);
  }
  .sides div {
    display: grid;
    grid-template-columns: 1.4em 1fr;
    gap: 6px;
  }
  .sides dt {
    font-family: var(--font-display);
    font-weight: 600;
    color: var(--porcelain);
  }
  .sides dd {
    margin: 0;
    color: var(--mist);
    overflow-wrap: anywhere;
  }

  .state {
    font-size: var(--t-sm);
    line-height: 1.45;
    color: var(--mist);
  }
  .state.bad {
    color: var(--alarm);
  }
  .wait {
    height: 2px;
    border-radius: 1px;
    background: linear-gradient(90deg, transparent, var(--porcelain), transparent) 0 0 / 40% 100% no-repeat var(--slate-600);
    animation: wait 1.2s linear infinite;
  }
  @keyframes wait {
    from {
      background-position: -70% 0;
    }
    to {
      background-position: 170% 0;
    }
  }

  /* The three keys are the test: big enough to hit without looking, X set apart as the
     one nobody knows. */
  .keys {
    display: grid;
    grid-template-columns: 1fr 1.25fr 1fr;
    gap: 10px;
  }
  .choice {
    height: 92px;
    border-radius: var(--r-lg);
    border: 1px solid var(--slate-500);
    background: var(--slate-700);
    color: var(--mist);
    font-family: var(--font-display);
    font-size: var(--t-3xl);
    font-weight: 600;
    line-height: 1;
    transition:
      background 120ms,
      color 120ms,
      border-color 120ms;
  }
  .choice.unknown {
    border-style: dashed;
  }
  .choice:hover {
    color: var(--porcelain);
  }
  .choice.on {
    background: var(--porcelain);
    border-color: var(--porcelain);
    border-style: solid;
    color: var(--ink);
  }
  .choice:focus-visible,
  .pause:focus-visible {
    outline: 2px solid var(--porcelain);
    outline-offset: 2px;
  }

  .transport {
    display: grid;
    grid-template-columns: auto auto 1fr auto;
    align-items: center;
    gap: 10px;
  }
  .pause {
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    border-radius: 50%;
    border: 1px solid var(--slate-500);
    color: var(--porcelain);
  }
  .time {
    font-size: var(--t-xs);
    font-variant-numeric: tabular-nums;
    color: var(--mist);
  }
  input[type="range"] {
    appearance: none;
    width: 100%;
    height: 14px;
    background: transparent;
  }
  input[type="range"]::-webkit-slider-runnable-track {
    height: 3px;
    border-radius: 2px;
    background: linear-gradient(to right, var(--porcelain) var(--at), var(--slate-600) var(--at));
  }
  input[type="range"]::-webkit-slider-thumb {
    appearance: none;
    width: 11px;
    height: 11px;
    margin-top: -4px;
    border-radius: 50%;
    background: var(--porcelain);
  }
  input[type="range"]::-moz-range-track {
    height: 3px;
    border-radius: 2px;
    background: var(--slate-600);
  }
  input[type="range"]::-moz-range-thumb {
    width: 11px;
    height: 11px;
    border: 0;
    border-radius: 50%;
    background: var(--porcelain);
  }

  .ask {
    display: grid;
    gap: 10px;
  }
  .trial {
    font-size: var(--t-sm);
    color: var(--mist);
    font-variant-numeric: tabular-nums;
  }
  .answers {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
  }
  .primary {
    padding: 10px 16px;
    border-radius: var(--r-md);
    background: var(--porcelain);
    color: var(--ink);
    font-weight: 600;
  }
  .primary:hover {
    filter: brightness(1.08);
  }
  .primary:disabled {
    opacity: 0.6;
  }
  .ghost {
    justify-self: start;
    padding: 8px 16px;
    border-radius: var(--r-md);
    border: 1px solid var(--slate-500);
    color: var(--porcelain);
    font-weight: 500;
  }
  .ghost:hover {
    background: var(--slate-700);
  }
  .count {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: var(--t-sm);
    color: var(--mist);
  }
  .seg {
    display: inline-flex;
    padding: 2px;
    border-radius: var(--r-md);
    background: var(--slate-700);
  }
  .seg button {
    min-width: 38px;
    padding: 4px 8px;
    border-radius: 6px;
    color: var(--mist);
    font-variant-numeric: tabular-nums;
  }
  .seg button.sel {
    background: var(--slate-500);
    color: var(--porcelain);
  }
  .how {
    font-size: var(--t-sm);
    line-height: 1.45;
    color: var(--mist);
  }

  .result {
    display: grid;
    gap: 6px;
  }
  .score {
    font-family: var(--font-display);
    font-size: var(--t-2xl);
    font-weight: 600;
    line-height: 1.1;
    font-variant-numeric: tabular-nums;
  }
  .verdict {
    font-size: var(--t-md);
    line-height: 1.4;
  }
  .chance {
    font-size: var(--t-sm);
    color: var(--mist);
  }
  .marks {
    margin: 6px 0 8px;
    padding: 0;
    list-style: none;
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .marks li {
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border-radius: var(--r-sm);
    background: var(--slate-700);
    font-size: var(--t-xs);
    font-weight: 600;
    color: var(--porcelain);
  }
  .marks li.wrong {
    background: transparent;
    box-shadow: inset 0 0 0 1px var(--alarm);
    color: var(--alarm);
  }

  .notes {
    margin: 0;
    padding: 12px 0 0 1.1em;
    border-top: 1px solid var(--slate-600);
    display: grid;
    gap: 5px;
    font-size: var(--t-xs);
    line-height: 1.45;
    color: var(--mist-dim);
  }

  @media (prefers-reduced-motion: reduce) {
    .wait {
      animation: none;
    }
    .choice {
      transition: none;
    }
  }
  @media (max-width: 420px) {
    .choice {
      height: 76px;
      font-size: var(--t-2xl);
    }
  }
</style>
