// Tauri doesn't have a Node.js server to do proper SSR
// so we use adapter-static with a fallback to index.html to put the site in SPA mode
// See: https://svelte.dev/docs/kit/single-page-apps
// See: https://v2.tauri.app/start/frontend/sveltekit/ for more info
import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

// VITE_DEMO=1 (`npm run build:demo`) builds the live demo for the website instead: served under
// /demo/, written to DEMO_OUT (the website's build sets it to its own dist/demo).
const demo = process.env.VITE_DEMO === "1";
const out = demo ? (process.env.DEMO_OUT ?? "build-demo") : "build";

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  kit: {
    adapter: adapter({
      pages: out,
      assets: out,
      fallback: "index.html",
    }),
    paths: { base: demo ? "/demo" : "" },
  },
};

export default config;
