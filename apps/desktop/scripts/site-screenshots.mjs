// The website's app screenshots (apps/website/public/app-classic.webp and app-glass.webp), taken
// from the desktop UI on its mock backend: `npm run dev`, then
// `node scripts/site-screenshots.mjs [url] [out dir]`. Both show the same moment (Strings in
// Winter playing bit-perfect) so the site's Classic / Glass switch changes only the look.
// 1120×740 at 2×, saved as PNG; the website README has the cwebp step.
import { chromium } from "playwright-core";

const url = process.argv[2] ?? "http://127.0.0.1:1420";
const out = process.argv[3] ?? new URL("../../website/public", import.meta.url).pathname;
const size = { width: 1120, height: 740 };
// A desktop browser that is neither a Mac (no room for traffic lights) nor a phone.
const ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";

const browser = await chromium.launch({ channel: "chrome" }); // the installed Google Chrome
for (const look of ["classic", "glass"]) {
  const page = await browser.newPage({ viewport: size, deviceScaleFactor: 2, userAgent: ua, locale: "en-US" });
  await page.addInitScript((l) => localStorage.setItem("look", JSON.stringify(l)), look);
  const tab = (name) => page.getByRole("button", { name, exact: true }).first().click();
  await page.goto(url);
  await page.waitForTimeout(1500);

  await tab("Library");
  await page.getByRole("button").filter({ hasText: "Strings in Winter" }).first().click();
  await page.getByRole("button", { name: "Play", exact: true }).click();
  await page.waitForTimeout(500);
  // The mock starts with the equalizer on; off, the path reads bit-perfect.
  await tab("Equalizer");
  await page.getByRole("switch").first().evaluate((el) => el.click());
  await tab("Now Playing");

  await page.mouse.move(1, size.height - 5);
  await page.waitForTimeout(4000);
  await page.screenshot({ path: `${out}/app-${look}.png` });
  console.log(`app-${look}.png`);
  await page.close();
}
await browser.close();
