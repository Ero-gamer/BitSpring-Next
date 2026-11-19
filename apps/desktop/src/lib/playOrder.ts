// The order a shuffled queue plays in, and the way back to the order it was added in.
//
// Shuffling reorders the queue itself, once: what the queue shows is what plays, and the
// track after the current one is known ahead (the backend opens it early to join it on
// without a gap). Each entry remembers its place in the straight order, so switching
// shuffle off puts the queue back — also after tracks were added or removed meanwhile.

/** What happens at the end of a track: the queue stops after its last one, starts over, or the track plays again. */
export type Repeat = "off" | "all" | "one";

/** The repeat mode the key switches to next. */
export function nextRepeat(r: Repeat): Repeat {
  return r === "off" ? "all" : r === "all" ? "one" : "off";
}

/**
 * A random order for `length` entries, as indices into the present order. Entry `first`
 * (the one playing, or the one asked for) leads it; pass -1 to shuffle them all.
 */
export function shuffledOrder(length: number, first: number, random: () => number = Math.random): number[] {
  const order: number[] = [];
  for (let i = 0; i < length; i++) if (i !== first) order.push(i);
  // Fisher–Yates: every order is equally likely.
  for (let i = order.length - 1; i > 0; i--) {
    const j = Math.floor(random() * (i + 1));
    [order[i], order[j]] = [order[j], order[i]];
  }
  if (first >= 0 && first < length) order.unshift(first);
  return order;
}

/** The order that puts entries back by their `places` in the straight order (indices into the present order). */
export function straightOrder(places: number[]): number[] {
  return places.map((_, i) => i).sort((a, b) => places[a] - places[b]);
}
