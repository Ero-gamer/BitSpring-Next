// Rebuilds src/releases.json, the data behind /releases/, from the GitHub releases of the app.
// Each entry keeps the "What's new" part of the release notes; the install instructions and
// checksums that follow it stay on GitHub. Run: npm run releases (GH_TOKEN is used if set).
// The releases workflow runs it whenever a release is published or edited.
import { writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const REPO = "crash2bash/bitspring";
const out = fileURLToPath(new URL("../src/releases.json", import.meta.url));

const headers = { Accept: "application/vnd.github+json", "User-Agent": "bitspring-site" };
if (process.env.GH_TOKEN) headers.Authorization = `Bearer ${process.env.GH_TOKEN}`;

const res = await fetch(`https://api.github.com/repos/${REPO}/releases?per_page=100`, { headers });
if (!res.ok) throw new Error(`GitHub answered ${res.status}: ${await res.text()}`);
const releases = await res.json();

// Where the "What's new" part ends: the update hint, the per-platform install notes, the
// checksums, or a repeat of an earlier release's notes ("From 0.6.0:").
const END = [
  /^(BitSpring|Hifi) (updates itself|0\.3\.0 (and later )?offers?)/,
  /^\*\*(Mac|Windows|Android|Linux|iPhone|SHA-256)\*\*/,
  /^The iPhone build/,
  /^\| File/,
  /^From \d+\.\d+\.\d+:/,
];

function notesOf(body) {
  const lines = body.replace(/\r\n/g, "\n").split("\n");
  const start = lines.findIndex((l) => /^\*\*What's (new|fixed)\*\*/.test(l.trim()));
  if (start < 0) {
    // The first release had no such section, only an opening sentence.
    const first = lines[0].replace(/\s*Built from \w+\.?\s*$/, "").trim();
    return first;
  }
  const kept = [];
  for (const line of lines.slice(start + 1)) {
    if (END.some((re) => re.test(line.trim()))) break;
    kept.push(line);
  }
  return kept.join("\n").trim();
}

const data = releases
  .filter((r) => !r.draft && !r.prerelease && /^v\d+\.\d+\.\d+$/.test(r.tag_name))
  .map((r) => ({
    version: r.tag_name.slice(1),
    name: r.name,
    date: r.published_at,
    url: r.html_url,
    notes: notesOf(r.body ?? ""),
  }))
  .sort((a, b) => b.date.localeCompare(a.date));

writeFileSync(out, JSON.stringify(data, null, 2) + "\n");
console.log(`${data.length} releases, newest ${data[0]?.version} → src/releases.json`);
