// The website's app screenshots (apps/website/public/screens/<screen>-<look>.webp), behind the
// home page's tabs: Now Playing bit-perfect ("player") and with the equalizer on
// ("processing"), the library and the equalizer, each in the glass and the classic look.
// Taken from the desktop UI on its mock backend, looking like a store build (?store: no
// streaming tab, no updater): `npm run dev`, then `node scripts/site-screenshots.mjs [url] [out]`.
// 1120×740 at 2×, saved as PNG; the website README has the cwebp step.
import { chromium } from "playwright-core";

const url = process.argv[2] ?? "http://127.0.0.1:1420";
const out = process.argv[3] ?? new URL("../../website/public/screens", import.meta.url).pathname;
const size = { width: 1120, height: 740 };
// A desktop browser that is neither a Mac (no room for traffic lights) nor a phone.
const ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";

const browser = await chromium.launch({ channel: "chrome" }); // the installed Google Chrome
for (const look of ["glass", "classic"]) {
  const page = await browser.newPage({ viewport: size, deviceScaleFactor: 2, userAgent: ua, locale: "en-US" });
  await page.addInitScript((l) => localStorage.setItem("look", JSON.stringify(l)), look);
  const tab = (name) => page.getByRole("button", { name, exact: true }).first().click();
  const shot = async (screen, wait = 1500) => {
    await page.mouse.move(1, size.height - 5);
    await page.waitForTimeout(wait);
    await page.screenshot({ path: `${out}/${screen}-${look}.png` });
    console.log(`${screen}-${look}.png`);
  };
  await page.goto(`${url}/?store`);
  await page.waitForTimeout(1500);

  await tab("Library");
  await page.getByRole("button").filter({ hasText: "Strings in Winter" }).first().click();
  await page.getByRole("button", { name: "Play", exact: true }).click();
  await page.waitForTimeout(500);

  // The mock starts with the equalizer on: its screen, and the player showing the processing.
  await tab("Equalizer");
  await shot("eq");
  await tab("Now Playing");
  await shot("processing", 4000);

  // Equalizer off: bit-perfect.
  await tab("Equalizer");
  await page.getByRole("switch").first().evaluate((el) => el.click());
  await tab("Now Playing");
  await shot("player", 4000);

  await tab("Library");
  await page.getByRole("button", { name: "Back" }).click().catch(() => {});
  await shot("library");
  await page.close();
}
await browser.close();
