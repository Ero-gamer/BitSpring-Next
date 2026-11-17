// The website's app screenshots (apps/website/public/screens/<screen>-<look>.webp), behind the
// home page's tabs: Now Playing bit-perfect ("player") and with the equalizer on
// ("processing"), the library and the equalizer, each in the glass and the classic look.
// Taken from the desktop UI on its mock backend, looking like a store build (?store: no
// streaming tab, no updater): `npm run dev`, then `node scripts/site-screenshots.mjs [url] [out]`.
// 1120×740 at 2×, saved as PNG; the website README has the cwebp step.
//
// `--phone` takes the same screens as the iPhone app shows them (<screen>-<look>-phone.png),
// which the site shows on phones: 393×852 points at 3× (iPhone 15/16 class), with the status
// bar, Dynamic Island and home indicator drawn over the app the way iOS lays them out.
// `--android` does the same for Android phones (<screen>-<look>-android.png): 412×915 dp at
// 2.625× (Pixel class), status bar, camera punch-hole and gesture bar.
import { chromium } from "playwright-core";

const args = process.argv.slice(2);
const kind = args.includes("--android") ? "android" : args.includes("--phone") ? "phone" : "desktop";
const phone = kind !== "desktop";
const [url = "http://127.0.0.1:1420", out = new URL("../../website/public/screens", import.meta.url).pathname] = args.filter(
  (a) => !a.startsWith("--"),
);

const DESKTOP = {
  size: { width: 1120, height: 740 },
  scale: 2,
  // A desktop browser that is neither a Mac (no room for traffic lights) nor a phone.
  ua: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36",
  suffix: "",
};
const PHONE = {
  size: { width: 393, height: 852 },
  scale: 3,
  ua: "Mozilla/5.0 (iPhone; CPU iPhone OS 26_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Mobile/15E148 Safari/604.1",
  suffix: "-phone",
};
const ANDROID = {
  size: { width: 412, height: 915 },
  scale: 2.625,
  ua: "Mozilla/5.0 (Linux; Android 16; Pixel 9) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Mobile Safari/537.36",
  suffix: "-android",
};
const device = { desktop: DESKTOP, phone: PHONE, android: ANDROID }[kind];

// iOS draws these over every app; the app keeps clear of them through --safe-top/--safe-bottom
// (the same variables its own iPhone build fills from env(safe-area-inset-*)).
const IOS_CHROME = `
  <div style="position:fixed;inset:0 0 auto 0;height:59px;z-index:2147483647;pointer-events:none;
    font:600 17px/1 -apple-system,'SF Pro Text',system-ui;color:#fff;letter-spacing:-0.2px">
    <span style="position:absolute;left:0;width:150px;top:21px;text-align:center">9:41</span>
    <span style="position:absolute;left:50%;top:11px;width:125px;height:37px;margin-left:-62.5px;border-radius:19px;background:#000"></span>
    <svg style="position:absolute;right:28px;top:21px" width="78" height="13" viewBox="0 0 78 13" fill="#fff">
      <rect x="0" y="8" width="3" height="4" rx="1"/><rect x="5" y="6" width="3" height="6" rx="1"/>
      <rect x="10" y="3.5" width="3" height="8.5" rx="1"/><rect x="15" y="1" width="3" height="11" rx="1"/>
      <path d="M31.5 3.2a9.6 9.6 0 0 1 13 0l-1.3 1.4a7.7 7.7 0 0 0-10.4 0zM34 5.9a6 6 0 0 1 8 0l-1.3 1.4a4.1 4.1 0 0 0-5.4 0zM36.6 8.6a2.2 2.2 0 0 1 2.8 0L38 10.2z"/>
      <rect x="51.5" y="0.5" width="22" height="12" rx="3.5" fill="none" stroke="#fff" stroke-opacity="0.4"/>
      <rect x="53.5" y="2.5" width="18" height="8" rx="2"/><path d="M75 4.5v4a2 2 0 0 0 0-4z" fill-opacity="0.4"/>
    </svg>
  </div>
  <div style="position:fixed;left:50%;bottom:8px;width:134px;height:5px;margin-left:-67px;border-radius:3px;
    background:rgba(255,255,255,0.8);z-index:2147483647;pointer-events:none"></div>`;

// Android's status bar (clock left, Wi-Fi/signal/battery right, the front camera's punch-hole
// in the middle) and the gesture bar.
const ANDROID_CHROME = `
  <div style="position:fixed;inset:0 0 auto 0;height:40px;z-index:2147483647;pointer-events:none;
    font:500 14px/1 Roboto,'Google Sans',system-ui;color:#fff">
    <span style="position:absolute;left:24px;top:14px">9:30</span>
    <span style="position:absolute;left:50%;top:9px;width:22px;height:22px;margin-left:-11px;border-radius:50%;background:#000"></span>
    <svg style="position:absolute;right:22px;top:13px" width="58" height="14" viewBox="0 0 58 14" fill="#fff">
      <path d="M8 13 0.6 4.6a10.6 10.6 0 0 1 14.8 0z"/>
      <path d="M19 13h12V1z"/>
      <rect x="38" y="2" width="18" height="10" rx="2.5" fill="none" stroke="#fff" stroke-opacity="0.5"/>
      <rect x="39.5" y="3.5" width="12" height="7" rx="1.2"/>
    </svg>
  </div>
  <div style="position:fixed;left:50%;bottom:10px;width:108px;height:4px;margin-left:-54px;border-radius:2px;
    background:rgba(255,255,255,0.75);z-index:2147483647;pointer-events:none"></div>`;
const SYSTEM_BARS = {
  phone: { top: 59, bottom: 34, html: IOS_CHROME },
  android: { top: 40, bottom: 24, html: ANDROID_CHROME },
}[kind];

const browser = await chromium.launch({ channel: "chrome" }); // the installed Google Chrome
for (const look of ["glass", "classic"]) {
  const page = await browser.newPage({
    viewport: device.size,
    deviceScaleFactor: device.scale,
    userAgent: device.ua,
    locale: "en-US",
    hasTouch: phone,
    isMobile: phone,
  });
  await page.addInitScript((l) => localStorage.setItem("look", JSON.stringify(l)), look);
  const tab = (name) => page.getByRole("button", { name, exact: true }).first().click();
  const shot = async (screen, wait = 1500) => {
    if (!phone) await page.mouse.move(1, device.size.height - 5);
    await page.waitForTimeout(wait);
    const file = `${screen}-${look}${device.suffix}.png`;
    await page.screenshot({ path: `${out}/${file}` });
    console.log(file);
  };
  await page.goto(`${url}/?store`);
  if (phone) {
    await page.evaluate(({ top, bottom, html }) => {
      document.documentElement.style.setProperty("--safe-top", `${top}px`);
      document.documentElement.style.setProperty("--safe-bottom", `${bottom}px`);
      document.body.insertAdjacentHTML("beforeend", html);
    }, SYSTEM_BARS);
  }
  await page.waitForTimeout(1500);

  // The glass backdrop is made from the playing cover, so the album sets the mood of every glass
  // shot. Quiet Hours (teal sky, warm sun) gives it colour without brass or violet, which mean
  // something; a pale blue cover like Strings in Winter blurs down to flat navy and the glass
  // disappears. Classic plays the same album so the site's look switch changes only the look.
  await tab("Library");
  await page.getByRole("button").filter({ hasText: "Quiet Hours" }).first().click();
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
