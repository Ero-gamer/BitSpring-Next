// Build-time markup for /releases/ (runs in Node, from render.ts). The notes come from
// src/releases.json (made by scripts/releases.mjs from the GitHub releases) and stay in English
// in every language; dates and the page around them are translated.

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import type { Locale } from "./locales";

interface Release {
  version: string;
  name: string;
  /** ISO time the release was published. */
  date: string;
  url: string;
  /** Markdown: "- " items (two spaces deeper for sub-items) and plain paragraphs. */
  notes: string;
}

const ISSUES = "https://github.com/crash2bash/bitspring/issues/";

const escapeHtml = (s: string) =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

/** `code`, **bold**, [text](url) and "#6" issue numbers, the only markup the notes use. */
function inline(src: string): string {
  return escapeHtml(src)
    .replace(/`([^`]+)`/g, "<code>$1</code>")
    .replace(/\*\*(.+?)\*\*/g, "<strong>$1</strong>")
    .replace(/\[([^\]]+)\]\((https:[^)\s]+)\)/g, '<a href="$2" rel="noopener">$1</a>')
    .replace(/\(#(\d+)\)/g, `(<a href="${ISSUES}$1" rel="noopener">#$1</a>)`);
}

function notesHtml(md: string): string {
  const out: string[] = [];
  let depth = 0; // open <ul> levels
  const close = (to: number) => {
    for (; depth > to; depth--) out.push("</li></ul>");
  };
  for (const raw of md.split("\n")) {
    if (!raw.trim()) continue;
    const item = raw.match(/^(\s*)- (.*)$/);
    if (!item) {
      close(0);
      out.push(`<p>${inline(raw.trim())}</p>`);
      continue;
    }
    const level = item[1].length >= 2 ? 2 : 1;
    if (level > depth) {
      for (; depth < level; depth++) out.push("<ul>");
      out.push("<li>");
    } else {
      close(level);
      out.push("</li><li>");
    }
    out.push(inline(item[2]));
  }
  close(0);
  return out.join("");
}

let cache: Release[] | undefined;
const load = (): Release[] =>
  (cache ??= JSON.parse(readFileSync(fileURLToPath(new URL("../releases.json", import.meta.url)), "utf8")));

/** The timeline: one group per day (UTC, as GitHub dates them), newest first. */
export function releasesHtml(locale: Locale, t: (key: string) => string): string {
  const day = new Intl.DateTimeFormat(locale.tag, { day: "numeric", month: "long", year: "numeric", timeZone: "UTC" });
  const days = new Map<string, Release[]>();
  for (const r of load()) {
    const d = r.date.slice(0, 10);
    days.set(d, [...(days.get(d) ?? []), r]);
  }

  const release = (r: Release) => {
    const oldName = r.name.startsWith("Hifi ") ? `<span class="rel-was">${escapeHtml(t("releases.as_hifi"))}</span>` : "";
    return `<li class="rel" id="v${r.version}">
            <h3 class="rel-version"><a href="#v${r.version}">${r.version}</a>${oldName}</h3>
            <div class="rel-notes" lang="en" dir="ltr">${notesHtml(r.notes)}</div>
            <a class="rel-files" href="${escapeHtml(r.url)}" rel="noopener">${escapeHtml(t("releases.files"))}</a>
          </li>`;
  };

  return [...days]
    .map(
      ([d, rs]) => `<li class="rel-day">
        <h2 class="rel-date"><time datetime="${d}">${escapeHtml(day.format(new Date(`${d}T12:00:00Z`)))}</time></h2>
        <ol class="rel-list">
          ${rs.map(release).join("\n          ")}
        </ol>
      </li>`,
    )
    .join("\n      ");
}
