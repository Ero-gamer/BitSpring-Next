// Folder paths in the user's words: on a phone "Internal storage / Music", not
// /storage/emulated/0/Music.
import { isAndroid } from "./api";
import { t } from "./i18n.svelte";

export const folderParts = (path: string) => path.split(/[\\/]+/).filter(Boolean);
export const folderName = (path: string) => folderParts(path).at(-1) ?? path;

/** The folder a path is in; a root stays itself. */
export function parentOf(path: string): string {
  const cut = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  if (cut < 0) return path;
  return cut === 0 ? path[0] : path.slice(0, cut);
}

/** Whether `path` is `folder` or something inside it ("/a/bc" is not inside "/a/b"). */
export function isInside(path: string, folder: string): boolean {
  if (path === folder) return true;
  const f = /[\\/]$/.test(folder) ? folder : folder + (folder.includes("\\") ? "\\" : "/");
  return path.startsWith(f);
}

/** Android: the storage a path is on (internal, or a card or USB drive by its id), and
 *  the folders below that storage's root. Null for any other path. */
export function volumeOf(path: string): { key: string; internal: boolean; inside: string[] } | null {
  const m = /^\/storage\/(emulated\/\d+|[^/]+)(\/.*)?$/.exec(path);
  if (!m) return null;
  return { key: m[1], internal: m[1].startsWith("emulated/"), inside: folderParts(m[2] ?? "") };
}

export const volumeLabel = (v: { internal: boolean }) => t(v.internal ? "lib.internalStorage" : "lib.removableStorage");

/** A whole path as one line. */
export function folderLabel(path: string): string {
  const v = isAndroid ? volumeOf(path) : null;
  return v ? [volumeLabel(v), ...v.inside].join(" / ") : path;
}
