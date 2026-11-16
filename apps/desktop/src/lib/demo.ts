// The live demo on the website (bitspring.app/demo/, inside the home page): this UI on its mock
// backend, playing real music through the browser. Built with VITE_DEMO=1 (`npm run build:demo`);
// `?demo` turns it on in the dev server too.
//
// Imported first by +layout.svelte: the storage swap below has to happen before the look and the
// language read their saved values.
import type { EqPreset } from "./api";

const params = typeof location !== "undefined" ? new URLSearchParams(location.search) : new URLSearchParams();
export const isDemo = import.meta.env.VITE_DEMO === "1" || params.has("demo");

// ---- settings live in memory ---------------------------------------------------------------
// The demo shares its origin with the website, so it must not leave its device, queue and look
// choices in the site's storage. Every visit starts fresh, with the site's language and look.
if (isDemo && typeof window !== "undefined") {
  const mem = new Map<string, string>();
  const storage: Storage = {
    get length() {
      return mem.size;
    },
    clear: () => mem.clear(),
    getItem: (k) => mem.get(k) ?? null,
    key: (i) => [...mem.keys()][i] ?? null,
    removeItem: (k) => void mem.delete(k),
    setItem: (k, v) => void mem.set(k, String(v)),
  };
  const lang = params.get("lang");
  if (lang) storage.setItem("language", lang);
  const look = params.get("look");
  if (look) storage.setItem("look", JSON.stringify(look));
  Object.defineProperty(window, "localStorage", { value: storage, configurable: true });
}

// ---- the music -------------------------------------------------------------------------------
// Haydn, String Quartet in D, Op. 64 No. 5 "Lark": the Musopen recording (public domain), one
// excerpt per movement. Every other track in the mock library plays one of them too.
// The excerpts are the website's files (apps/website/public/demo-audio); in development they
// come from its dev server.
const AUDIO_BASE: string = import.meta.env.DEV ? "http://127.0.0.1:5180/demo-audio/" : "/demo-audio/";
export const DEMO_CLIPS = [
  { title: "I. Allegro moderato", seconds: 45 },
  { title: "II. Adagio cantabile", seconds: 45 },
  { title: "III. Menuetto. Allegretto", seconds: 40 },
  { title: "IV. Finale. Vivace", seconds: 40 },
];
export const DEMO_ALBUM = { artist: "Joseph Haydn", title: "String Quartet Op. 64 No. 5 “Lark”", year: "1790" };

function clipIndex(path: string): number {
  const own = DEMO_CLIPS.findIndex((c) => path.includes(`/${DEMO_ALBUM.title}/`) && path.includes(c.title));
  if (own >= 0) return own;
  let h = 0;
  for (const ch of path) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
  return h % DEMO_CLIPS.length;
}
export const clipSeconds = (path: string) => DEMO_CLIPS[clipIndex(path)].seconds;

// ---- the player -----------------------------------------------------------------------------
// <audio> → EQ (the active preset's real filters, per channel) → meters → speakers.

let ctx: AudioContext | null = null;
let el: HTMLAudioElement | null = null;
let source: MediaElementAudioSourceNode | null = null;
let output: GainNode | null = null;
let analysers: [AnalyserNode, AnalyserNode] | null = null;
let chain: AudioNode[] = [];
let chainKey = "";
let eqNow: EqPreset | null = null;
let volume = 1;
const buf = new Float32Array(2048);

function setup() {
  if (ctx) return;
  ctx = new AudioContext();
  el = new Audio();
  el.crossOrigin = "anonymous";
  // Nothing downloads until Play: most visitors only look around (and the site's bandwidth is free-tier).
  el.preload = "none";
  source = ctx.createMediaElementSource(el);
  output = ctx.createGain();
  output.gain.value = volume;
  const split = ctx.createChannelSplitter(2);
  analysers = [ctx.createAnalyser(), ctx.createAnalyser()];
  output.connect(ctx.destination);
  output.connect(split);
  analysers.forEach((a, i) => {
    a.fftSize = 2048;
    split.connect(a, i);
  });
  wire();
}

// Browsers only start sound from a click: the first one anywhere in the demo unlocks it.
if (isDemo && typeof window !== "undefined") {
  window.addEventListener(
    "pointerdown",
    () => {
      setup();
      ctx?.resume();
    },
    { once: true, capture: true },
  );
}

const FILTER: Record<string, BiquadFilterType> = {
  peak: "peaking", lowShelf: "lowshelf", highShelf: "highshelf", lowPass: "lowpass",
  highPass: "highpass", bandPass: "bandpass", notch: "notch", allPass: "allpass",
};
const db = (v: number) => Math.pow(10, v / 20);

/** One channel's filters for a preset: the bands meant for it, steeper passes as cascades. */
function channelFilters(p: EqPreset, ch: "left" | "right"): BiquadFilterNode[] {
  const out: BiquadFilterNode[] = [];
  for (const b of p.bands) {
    if (!b.enabled || (b.channels !== "both" && b.channels !== ch)) continue;
    const passes = b.kind === "lowPass" || b.kind === "highPass" ? Math.max(1, Math.round(b.slope / 12)) : 1;
    for (let i = 0; i < passes; i++) {
      const f = ctx!.createBiquadFilter();
      f.type = FILTER[b.kind] ?? "peaking";
      f.frequency.value = Math.min(b.freq, ctx!.sampleRate / 2 - 100);
      f.Q.value = b.q;
      f.gain.value = b.gain;
      out.push(f);
    }
  }
  return out;
}

const shape = (p: EqPreset | null) =>
  p ? p.bands.map((b) => `${b.enabled}${b.kind}${b.channels}${b.slope}`).join("|") : "";

/** Rebuilds the graph when the bands change shape; while a slider moves, only values change. */
function wire() {
  if (!ctx || !source || !output) return;
  const key = shape(eqNow);
  if (key === chainKey && eqNow && chain.length) {
    updateValues(eqNow);
    return;
  }
  source.disconnect();
  chain.forEach((n) => n.disconnect());
  chain = [];
  chainKey = key;
  if (!eqNow) {
    source.connect(output);
    return;
  }
  const split = ctx.createChannelSplitter(2);
  const merge = ctx.createChannelMerger(2);
  chain.push(split, merge);
  source.connect(split);
  (["left", "right"] as const).forEach((ch, i) => {
    const gain = ctx!.createGain();
    const filters = channelFilters(eqNow!, ch);
    chain.push(gain, ...filters);
    split.connect(gain, i);
    let last: AudioNode = gain;
    for (const f of filters) {
      last.connect(f);
      last = f;
    }
    last.connect(merge, 0, i);
  });
  merge.connect(output);
  updateValues(eqNow);
}

function updateValues(p: EqPreset) {
  const t = ctx!.currentTime;
  const gains = chain.filter((n): n is GainNode => n instanceof GainNode);
  const balance = [Math.max(0, p.balance), Math.max(0, -p.balance)];
  gains.forEach((g, i) => g.gain.setTargetAtTime(db(p.preamp - balance[i]), t, 0.02));
  (["left", "right"] as const).forEach((ch, i) => {
    const fresh = channelFilters(p, ch);
    const start = chain.indexOf(gains[i]) + 1;
    fresh.forEach((f, k) => {
      const live = chain[start + k] as BiquadFilterNode;
      live.frequency.setTargetAtTime(f.frequency.value, t, 0.02);
      live.Q.setTargetAtTime(f.Q.value, t, 0.02);
      live.gain.setTargetAtTime(f.gain.value, t, 0.02);
    });
  });
}

export const demoAudio = {
  load(path: string, startAt: number, paused: boolean) {
    setup();
    const src = `${AUDIO_BASE}${clipIndex(path) + 1}.m4a`;
    if (!el!.src.endsWith(src)) el!.src = src;
    el!.currentTime = startAt;
    if (paused) el!.pause();
    else {
      ctx!.resume();
      el!.play().catch(() => {});
    }
  },
  setPaused(p: boolean) {
    if (!el) return;
    if (p) el.pause();
    else {
      ctx?.resume();
      el.play().catch(() => {});
    }
  },
  seek(s: number) {
    if (el) el.currentTime = s;
  },
  stop() {
    el?.pause();
  },
  get position() {
    return el?.currentTime ?? 0;
  },
  get ended() {
    return !!el?.ended;
  },
  setEq(p: EqPreset | null) {
    eqNow = p ? structuredClone(p) : null;
    wire();
  },
  setVolume(v: number) {
    volume = Math.pow(Math.max(0, Math.min(100, v)) / 100, 1.6);
    if (ctx && output) output.gain.setTargetAtTime(volume, ctx.currentTime, 0.03);
  },
  /** Peak level of the last ~40 ms per channel, in dBFS. */
  levels(): [number, number] {
    if (!analysers || !el || el.paused) return [-90, -90];
    return analysers.map((a) => {
      a.getFloatTimeDomainData(buf);
      let peak = 0;
      for (const s of buf) peak = Math.max(peak, Math.abs(s));
      return peak > 0 ? Math.max(-90, 20 * Math.log10(peak)) : -90;
    }) as [number, number];
  },
};
