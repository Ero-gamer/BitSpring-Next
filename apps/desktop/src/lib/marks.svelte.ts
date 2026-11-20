// What the listener marked: favourite tracks and albums, and ratings. Kept by the backend
// (on this device only); a change made here shows at once and is then confirmed by it.
import { api, serviceOf, type Mark } from "./api";

const RATINGS_KEY = "ratings";

function loadRatings(): boolean {
  try {
    return JSON.parse(localStorage.getItem(RATINGS_KEY) ?? "false") === true;
  } catch {
    return false;
  }
}

const state = $state<{ tracks: Record<string, Mark>; albums: Record<string, unknown>; ratings: boolean }>({
  tracks: {},
  albums: {},
  ratings: typeof localStorage !== "undefined" ? loadRatings() : false,
});
/** The backend's revision we show (see Marks.rev). */
let rev = -1;

export const marks = {
  fav: (path: string): boolean => state.tracks[path]?.fav ?? false,
  /** 1 to 5 stars; 0: not rated. */
  rating: (path: string): number => state.tracks[path]?.rating ?? 0,
  album: (id: string): boolean => id in state.albums,
  /** Whether the stars show: off until the listener asks for them in Settings (per machine).
   *  The hearts are always there. */
  get ratings(): boolean {
    return state.ratings;
  },
  set ratings(v: boolean) {
    state.ratings = v;
    try {
      localStorage.setItem(RATINGS_KEY, JSON.stringify(v));
    } catch {}
  },
};

/** A track that can carry a mark: a file, not a streaming service's track (those have that
 *  service's own favourites) and not a playlist line whose file was never found. */
export const markable = (path: string | null | undefined): path is string => !!path && !serviceOf(path) && !path.startsWith("missing:");

/** Fetch everything marked: at the start, and when the backend says the marks changed
 *  without us (the phone remote, the API, a favourite album renamed by a scan). */
export async function loadMarks() {
  try {
    const all = await api.marks();
    rev = all.rev;
    state.tracks = all.tracks;
    state.albums = all.albums;
  } catch {
    // (Nothing to show yet; the next change or announcement asks again.)
  }
}

/** Wait for the backend's word on a change shown already. Its revision is the one after
 *  ours (or ours: nothing changed) unless something else changed meanwhile — or the change
 *  couldn't be saved: then what it has is fetched and shown instead. */
async function settle(change: Promise<number>) {
  try {
    const r = await change;
    if (r === rev || r === rev + 1) rev = r;
    else await loadMarks();
  } catch {
    await loadMarks();
  }
}

function put(path: string, m: Mark) {
  if (m.fav || m.rating) state.tracks[path] = m;
  else delete state.tracks[path];
}

export function setFavorite(path: string, on: boolean) {
  if (!markable(path)) return;
  const m = state.tracks[path] ?? { fav: false, rating: 0, at: 0 };
  put(path, { ...m, fav: on, at: on ? Math.floor(Date.now() / 1000) : 0 });
  return settle(api.markFavorite(path, on));
}

/** `rating`: 1 to 5; 0 takes the stars away. */
export function setRating(path: string, rating: number) {
  if (!markable(path)) return;
  const m = state.tracks[path] ?? { fav: false, rating: 0, at: 0 };
  put(path, { ...m, rating });
  return settle(api.markRating(path, rating));
}

export function setAlbumFavorite(id: string, on: boolean) {
  if (on) state.albums[id] = { at: Math.floor(Date.now() / 1000) };
  else delete state.albums[id];
  return settle(api.markAlbum(id, on));
}
