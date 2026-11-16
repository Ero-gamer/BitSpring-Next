// The website's app screenshots (apps/website/public/app-classic.webp and app-glass.webp): the
// website demo's own opening view (`?demo`: Haydn's "Lark" quartet, no streaming tab) a few
// seconds into playback, in each look, so the screenshot and the live demo that replaces it
// match. Needs the desktop UI's dev server (`npm run dev`) and the website's (its dev server
// serves the demo audio). Then `node scripts/site-screenshots.mjs [url] [out dir]`.
// 1120×740 at 2×, saved as PNG; the website README has the cwebp step.
import { chromium } from "playwright-core";

const url = process.argv[2] ?? "http://127.0.0.1:1420";
const out = process.argv[3] ?? new URL("../../website/public", import.meta.url).pathname;
const size = { width: 1120, height: 740 };
// A desktop browser that is neither a Mac (no room for traffic lights) nor a phone.
const ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";

const browser = await chromium.launch({ channel: "chrome", args: ["--autoplay-policy=no-user-gesture-required"] });
for (const look of ["classic", "glass"]) {
  const page = await browser.newPage({ viewport: size, deviceScaleFactor: 2, userAgent: ua, locale: "en-US" });
  await page.goto(`${url}/?demo&lang=en&look=${look}`);
  await page.waitForTimeout(1500);
  await page.getByRole("button", { name: "Play", exact: true }).click();
  await page.mouse.move(1, size.height - 5);
  await page.waitForTimeout(4500);
  await page.screenshot({ path: `${out}/app-${look}.png` });
  console.log(`app-${look}.png`);
  await page.close();
}
await browser.close();
