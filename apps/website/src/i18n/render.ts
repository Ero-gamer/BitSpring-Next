// Build-time page rendering (runs in Node, from vite.config.ts). index.html is a template:
//   {{key}}          → the translated string, HTML-escaped (safe in text and attributes)
//   {{@home}}        → this language's home URL; {{@download}} → its download page
//   <!--@name-->     → generated markup (alternate links, language options, …)
// Every language gets a real static page, so search engines and link previews see translated
// text without running any JavaScript.

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { DEFAULT_LOCALE, LOCALES, localeUrl, type Locale } from "./locales";
import { releasesHtml } from "./releases";

type Dict = Record<string, string>;

/** Strings main.ts needs at runtime; shipped inline so there is no extra request. */
const RUNTIME_KEYS = /^(cta\.|dl\.|rates\.|app\.(alt_|state_))/;

const dir = fileURLToPath(new URL("./locales/", import.meta.url));

export function loadDict(l: Locale): Dict {
  return JSON.parse(readFileSync(`${dir}${l.path || "en"}.json`, "utf8"));
}

const escapeHtml = (s: string) =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

const escapeJson = (v: unknown) => JSON.stringify(v).replace(/</g, "\\u003c");

/** Absolute URL when the site origin is known (hreflang and og:url want absolute ones). */
const abs = (site: string, path: string) => (site ? site.replace(/\/$/, "") + path : path);

// Runs before first paint on the root page only: send visitors to their language unless they
// already picked one in the switcher. Mirrors matchLocale() in locales.ts. The page leaving is
// hidden: its scripts still run until the next one arrives, and its opening animation would
// play once here and again there.
function redirectScript(page: string): string {
  const paths = LOCALES.map((l) => l.path).filter(Boolean);
  return `<script>(function(){var P=${escapeJson(paths)};function m(t){t=t.toLowerCase();var l=t.split("-")[0];if(l==="zh")return /-(hant|tw|hk|mo)\\b/.test(t)?"zh-hant":"zh-hans";if(l==="no"||l==="nn")l="nb";if(l==="pt")return"pt-br";if(l==="en")return"";return P.indexOf(l)>=0?l:null}var p=null;try{p=localStorage.getItem("lang")}catch(e){}if(p===null){var ls=navigator.languages||[navigator.language||""];for(var i=0;i<ls.length&&p===null;i++)p=m(ls[i]||"")}if(p&&P.indexOf(p)>=0){document.documentElement.style.visibility="hidden";location.replace("/"+p+"/"+${escapeJson(page)}+location.search+location.hash)}})()</script>`;
}

// schema.org data for the home pages: WebSite gives Google the site name it shows above the
// result, SoftwareApplication says this is a free app and for which systems. iOS joins the list
// once the App Store build is out. No version number: the site isn't rebuilt on app releases.
function structuredData(
  site: string,
  locale: Locale,
  pageUrl: string,
  description: string,
  faq: { q: string; a: string }[],
): string {
  const home = abs(site, "/");
  const data = {
    "@context": "https://schema.org",
    "@graph": [
      { "@type": "WebSite", "@id": `${home}#website`, name: "BitSpring", url: home, inLanguage: locale.tag },
      {
        "@type": "SoftwareApplication",
        "@id": `${home}#app`,
        name: "BitSpring",
        url: pageUrl,
        description,
        inLanguage: locale.tag,
        applicationCategory: "MultimediaApplication",
        applicationSubCategory: "Music player",
        operatingSystem: "macOS, Windows, Linux, Android",
        downloadUrl: abs(site, localeUrl(locale) + "download/"),
        image: abs(site, "/og.png"),
        screenshot: abs(site, "/screens/player-classic.webp"),
        isAccessibleForFree: true,
        license: "https://opensource.org/licenses/MIT",
        offers: { "@type": "Offer", price: "0", priceCurrency: "USD" },
        sameAs: ["https://github.com/crash2bash/bitspring"],
      },
      // The questions in the page's FAQ section, word for word.
      {
        "@type": "FAQPage",
        "@id": `${pageUrl}#faq`,
        inLanguage: locale.tag,
        mainEntity: faq.map(({ q, a }) => ({ "@type": "Question", name: q, acceptedAnswer: { "@type": "Answer", text: a } })),
      },
    ],
  };
  return `<script type="application/ld+json">${escapeJson(data)}</script>`;
}

/** `page`: path below the language root, "" for home or "download/". */
export function renderPage(template: string, locale: Locale, opts: { site: string; page: string }): string {
  const url = (l: Locale) => localeUrl(l) + opts.page;
  const dict = loadDict(locale);
  const fallback = locale === DEFAULT_LOCALE ? dict : loadDict(DEFAULT_LOCALE);
  const t = (key: string) => {
    const v = dict[key] ?? fallback[key];
    if (v === undefined) throw new Error(`i18n: missing key "${key}" (${locale.tag})`);
    return v;
  };

  /** faq.q1/faq.a1, faq.q2/faq.a2, … for as many as the English dictionary has. */
  const faq = () => {
    const out: { q: string; a: string }[] = [];
    for (let i = 1; `faq.q${i}` in fallback; i++) out.push({ q: t(`faq.q${i}`), a: t(`faq.a${i}`) });
    return out;
  };

  const runtime: Dict = {};
  for (const k of Object.keys(fallback)) if (RUNTIME_KEYS.test(k)) runtime[k] = t(k);

  const blocks: Record<string, string> = {
    alternates: [
      ...LOCALES.map((l) => `<link rel="alternate" hreflang="${l.tag}" href="${abs(opts.site, url(l))}" />`),
      `<link rel="alternate" hreflang="x-default" href="${abs(opts.site, url(DEFAULT_LOCALE))}" />`,
      `<link rel="canonical" href="${abs(opts.site, url(locale))}" />`,
    ].join("\n    "),
    og: [
      `<meta property="og:url" content="${abs(opts.site, url(locale))}" />`,
      `<meta property="og:image" content="${abs(opts.site, "/og.png")}" />`,
      `<meta property="og:locale" content="${locale.tag.replace("-", "_")}" />`,
      `<meta property="og:site_name" content="BitSpring" />`,
      `<meta name="twitter:card" content="summary_large_image" />`,
      ...(opts.page === "" ? [structuredData(opts.site, locale, abs(opts.site, url(locale)), t("meta.description"), faq())] : []),
    ].join("\n    "),
    redirect: locale === DEFAULT_LOCALE ? redirectScript(opts.page) : "",
    "lang-options": LOCALES.map(
      (l) =>
        `<option value="${l.path}" lang="${l.tag}"${l === locale ? " selected" : ""}>${escapeHtml(l.name)}</option>`,
    ).join(""),
    releases: opts.page === "releases/" ? releasesHtml(locale, t) : "",
    // The notes themselves are in English; say so on every other language's page.
    "releases-english": locale === DEFAULT_LOCALE ? "" : `<p class="rel-english">${escapeHtml(t("releases.english"))}</p>`,
    runtime: `<script type="application/json" id="i18n">${escapeJson({ numbers: locale.numbers, strings: runtime })}</script>`,
  };

  const computed: Record<string, string> = {
    home: localeUrl(locale),
    download: localeUrl(locale) + "download/",
    privacy: localeUrl(locale) + "privacy/",
    releases: localeUrl(locale) + "releases/",
  };

  return template
    .replace(
      /<html[^>]*>/,
      `<html lang="${locale.tag}" dir="${locale.dir}" data-font="${locale.font}" data-page="${opts.page}">`,
    )
    .replace(/<!--@([\w-]+)-->/g, (_, name: string) => {
      if (!(name in blocks)) throw new Error(`i18n: unknown block "${name}"`);
      return blocks[name];
    })
    .replace(/\{\{\s*@(\w+)\s*\}\}/g, (_, name: string) => {
      if (!(name in computed)) throw new Error(`i18n: unknown value "@${name}"`);
      return escapeHtml(computed[name]);
    })
    .replace(/\{\{\s*([\w.]+)\s*\}\}/g, (_, key: string) => escapeHtml(t(key)));
}
