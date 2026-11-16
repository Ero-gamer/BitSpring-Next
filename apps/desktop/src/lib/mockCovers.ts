// Cover art for the mock library's made-up albums. Abstract and drawn here, so screenshots (store
// listings, the website) show real-looking covers without anyone else's artwork in them.
// Browser-only, like mock.ts.

/** Same picture on every load: a small seeded PRNG (mulberry32). */
function rng(seed: number) {
  return () => {
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
const f = (n: number) => n.toFixed(1);
const svg = (body: string) =>
  `data:image/svg+xml;utf8,${encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" width="600" height="600" viewBox="0 0 300 300">${body}</svg>`)}`;
/** Fine paper/film grain over the whole cover. */
const grain = (id: string, alpha: number) =>
  `<filter id="${id}"><feTurbulence type="fractalNoise" baseFrequency="1.1" numOctaves="2" stitchTiles="stitch"/><feColorMatrix values="0 0 0 0 1 0 0 0 0 1 0 0 0 0 1 0 0 0 ${alpha} 0"/></filter>`;

// Folk, warm: a paper-cut crescent over layered hills, the moon's phases along the top.
function paperMoons(): string {
  const phases = [0, 1, 2, 3, 4]
    .map((i) => {
      const x = 62 + i * 44;
      const off = (i - 2) * 6;
      return `<mask id="pm${i}"><circle cx="${x}" cy="40" r="9" fill="#fff"/><circle cx="${x + off}" cy="40" r="9" fill="#000" opacity="${i === 2 ? 0 : 1}"/></mask><circle cx="${x}" cy="40" r="9" fill="#f1e4c3" mask="url(#pm${i})"/><circle cx="${x}" cy="40" r="9" fill="none" stroke="#f1e4c3" stroke-opacity=".35"/>`;
    })
    .join("");
  return svg(`<defs><linearGradient id="bg" x2="0" y2="1"><stop offset="0" stop-color="#7d6a37"/><stop offset="1" stop-color="#2c2413"/></linearGradient>
<mask id="cr"><circle cx="150" cy="138" r="62" fill="#fff"/><circle cx="176" cy="122" r="56" fill="#000"/></mask>
<filter id="sh"><feDropShadow dx="0" dy="3" stdDeviation="3" flood-color="#1a1408" flood-opacity=".55"/></filter>${grain("gr", 0.09)}</defs>
<rect width="300" height="300" fill="url(#bg)"/>${phases}
<circle cx="150" cy="138" r="62" fill="#f3e7c6" mask="url(#cr)" filter="url(#sh)"/>
<path d="M0 222 C60 196 110 206 160 220 S260 200 300 210 V300 H0Z" fill="#a1843f" filter="url(#sh)"/>
<path d="M0 248 C70 230 120 252 190 240 S270 232 300 244 V300 H0Z" fill="#7a6229" filter="url(#sh)"/>
<path d="M0 272 C80 262 150 280 220 266 S280 266 300 270 V300 H0Z" fill="#4b3c18" filter="url(#sh)"/>
<rect width="300" height="300" filter="url(#gr)"/>`);
}

// Electronic: three glowing carrier waves across a dark scope screen.
function signalPath(): string {
  const wave = (amp: number, freq: number, phase: number, y: number) => {
    let d = "";
    for (let x = 0; x <= 300; x += 3) {
      const env = Math.sin((x / 300) * Math.PI);
      d += `${x ? "L" : "M"}${x} ${f(y + Math.sin((x / 300) * freq * Math.PI * 2 + phase) * amp * env)}`;
    }
    return d;
  };
  const grid = Array.from({ length: 11 }, (_, i) => `<path d="M${i * 30} 0V300M0 ${i * 30}H300" stroke="#8fb3ff" stroke-opacity=".07"/>`).join("");
  const waves = [
    ["#4fe3d0", 46, 2.5, 0, 150],
    ["#ff5fa8", 34, 4, 1.2, 150],
    ["#ffc35a", 22, 7, 2.4, 150],
  ]
    .map(([c, a, fr, ph, y]) => `<path d="${wave(a as number, fr as number, ph as number, y as number)}" stroke="${c}" stroke-width="2.2" fill="none" filter="url(#glow)"/>`)
    .join("");
  return svg(`<defs><radialGradient id="bg" cx=".5" cy=".5" r=".75"><stop offset="0" stop-color="#1c2547"/><stop offset="1" stop-color="#090c18"/></radialGradient>
<filter id="glow" x="-10%" y="-30%" width="120%" height="160%"><feGaussianBlur stdDeviation="3" result="b"/><feMerge><feMergeNode in="b"/><feMergeNode in="SourceGraphic"/></feMerge></filter>${grain("gr", 0.06)}</defs>
<rect width="300" height="300" fill="url(#bg)"/>${grid}${waves}<rect width="300" height="300" filter="url(#gr)"/>`);
}

// Classical, cold: four strings (a quartet) in falling snow under a pale winter sun.
function stringsInWinter(): string {
  const r = rng(7);
  const snow = Array.from({ length: 90 }, () => {
    const s = 0.4 + r() * 1.8;
    return `<circle cx="${f(r() * 300)}" cy="${f(r() * 300)}" r="${f(s)}" fill="#fff" opacity="${f(0.25 + r() * 0.6)}"/>`;
  }).join("");
  const strings = [96, 128, 172, 204]
    .map((x, i) => `<path d="M${x} -5 Q${x + (i % 2 ? 4 : -4)} 150 ${x} 305" stroke="#e8f0ff" stroke-width="${1.6 - i * 0.25}" fill="none" opacity=".85" filter="url(#soft)"/>`)
    .join("");
  return svg(`<defs><linearGradient id="bg" x2="0" y2="1"><stop offset="0" stop-color="#14204a"/><stop offset=".65" stop-color="#3b5590"/><stop offset="1" stop-color="#9db4d8"/></linearGradient>
<radialGradient id="sun"><stop offset="0" stop-color="#fdfbf2" stop-opacity=".95"/><stop offset=".45" stop-color="#dfe8ff" stop-opacity=".35"/><stop offset="1" stop-color="#dfe8ff" stop-opacity="0"/></radialGradient>
<filter id="soft"><feGaussianBlur stdDeviation=".6"/></filter>${grain("gr", 0.05)}</defs>
<rect width="300" height="300" fill="url(#bg)"/><circle cx="150" cy="196" r="88" fill="url(#sun)"/>${strings}${snow}
<rect width="300" height="300" filter="url(#gr)"/>`);
}

// Jazz, dusk: a striped sun setting into still water.
function quietHours(): string {
  const stripes = [0, 1, 2, 3, 4].map((i) => `<rect x="60" y="${150 - 26 + i * 9 + i * i}" width="180" height="${1.5 + i * 1.2}" fill="#163644"/>`).join("");
  const r = rng(3);
  const reflect = Array.from({ length: 14 }, (_, i) => {
    const w = 120 - i * 7 + r() * 16;
    return `<rect x="${f(150 - w / 2 + (r() - 0.5) * 10)}" y="${168 + i * 8}" width="${f(w)}" height="${f(1.6 + r() * 1.6)}" rx="1" fill="#f0a868" opacity="${f(0.75 - i * 0.045)}"/>`;
  }).join("");
  return svg(`<defs><linearGradient id="sky" x2="0" y2="1"><stop offset="0" stop-color="#0d2530"/><stop offset=".6" stop-color="#2b5566"/><stop offset="1" stop-color="#d69563"/></linearGradient>
<linearGradient id="sea" x2="0" y2="1"><stop offset="0" stop-color="#1d3d4a"/><stop offset="1" stop-color="#0a1a21"/></linearGradient>
<linearGradient id="sun" x2="0" y2="1"><stop offset="0" stop-color="#ffd38e"/><stop offset="1" stop-color="#e9733f"/></linearGradient>
<clipPath id="above"><rect width="300" height="160"/></clipPath>${grain("gr", 0.06)}</defs>
<rect width="300" height="160" fill="url(#sky)"/><rect y="160" width="300" height="140" fill="url(#sea)"/>
<g clip-path="url(#above)"><circle cx="150" cy="160" r="72" fill="url(#sun)"/>${stripes}</g>${reflect}
<rect width="300" height="300" filter="url(#gr)"/>`);
}

// Jazz, night: city lights out of focus behind rain on glass.
function cityRain(): string {
  const r = rng(11);
  const colors = ["#ffb35c", "#ff6a4d", "#ffd27a", "#5fd0c8", "#e2584a"];
  const bokeh = Array.from({ length: 26 }, () => {
    const s = 8 + r() * 26;
    return `<circle cx="${f(r() * 300)}" cy="${f(40 + r() * 200)}" r="${f(s)}" fill="${colors[Math.floor(r() * colors.length)]}" opacity="${f(0.18 + r() * 0.4)}"/>`;
  }).join("");
  const rain = Array.from({ length: 70 }, () => {
    const x = r() * 320;
    const y = r() * 300;
    const l = 10 + r() * 26;
    return `<path d="M${f(x)} ${f(y)}l${f(-l * 0.18)} ${f(l)}" stroke="#dfe9f2" stroke-width="${f(0.5 + r() * 0.7)}" opacity="${f(0.2 + r() * 0.35)}"/>`;
  }).join("");
  return svg(`<defs><linearGradient id="bg" x2="0" y2="1"><stop offset="0" stop-color="#140d0c"/><stop offset=".7" stop-color="#4a2c21"/><stop offset="1" stop-color="#22140f"/></linearGradient>
<filter id="blur" x="-20%" y="-20%" width="140%" height="140%"><feGaussianBlur stdDeviation="6"/></filter>${grain("gr", 0.07)}</defs>
<rect width="300" height="300" fill="url(#bg)"/><g filter="url(#blur)">${bokeh}</g>${rain}
<rect y="250" width="300" height="50" fill="#0d0808" opacity=".55"/><rect width="300" height="300" filter="url(#gr)"/>`);
}

// Classical, surround: six sources around the listener, their rings overlapping.
function surroundSessions(): string {
  const src = [
    [150, 58], [246, 112], [246, 206], [150, 250], [54, 206], [54, 112],
  ];
  const rings = src
    .map(([x, y], i) =>
      [14, 30, 48, 68, 90]
        .map((rad, k) => `<circle cx="${x}" cy="${y}" r="${rad}" fill="none" stroke="${i % 2 ? "#c9b6ff" : "#ffffff"}" stroke-width="${1.4 - k * 0.2}" opacity="${f(0.5 - k * 0.09)}"/>`)
        .join("") + `<circle cx="${x}" cy="${y}" r="4" fill="#f4eeff"/>`,
    )
    .join("");
  return svg(`<defs><radialGradient id="bg" cx=".5" cy=".55" r=".75"><stop offset="0" stop-color="#4a3a8a"/><stop offset="1" stop-color="#160f33"/></radialGradient>
<radialGradient id="c"><stop offset="0" stop-color="#ffd27a" stop-opacity=".9"/><stop offset="1" stop-color="#ffd27a" stop-opacity="0"/></radialGradient>${grain("gr", 0.06)}</defs>
<rect width="300" height="300" fill="url(#bg)"/>${rings}<circle cx="150" cy="154" r="26" fill="url(#c)"/><circle cx="150" cy="154" r="5" fill="#ffe2a3"/>
<rect width="300" height="300" filter="url(#gr)"/>`);
}

/** Album id (mock.ts: a0, a1, …) → cover. Albums without one show the app's own placeholder. */
export const MOCK_COVERS: Record<string, () => string> = {
  a0: paperMoons,
  a1: signalPath,
  a2: stringsInWinter,
  a3: quietHours,
  a4: cityRain,
  a6: surroundSessions,
};
