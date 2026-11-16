// Playing from a game controller (a gamepad, a handheld's built-in controls): the
// backend reads the controller and sends each press as a `pad` event (see pad.rs).
//
//   D-pad / left stick   move the focus to the nearest control that way
//   A                    press it (a slider: nothing, left/right move it)
//   B                    back: close the open menu or dialog (as Escape)
//   X                    play / pause
//   LB / RB              previous / next track
//   LT / RT              10 s back / forward
//   right stick          scroll
//
// Focus shows a thick ring while the controller is in use (`data-pad` on <html>); the mouse
// or a touch takes it away again.

export type PadActions = {
  toggle(): void;
  next(): void;
  prev(): void;
  /** Seek by `delta` seconds from where playback is. */
  seekBy(delta: number): void;
};

type Dir = "up" | "down" | "left" | "right";

const FOCUSABLE =
  'button:not([disabled]), a[href], input:not([disabled]):not([type="hidden"]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"]), [role="button"], [role="option"], [role="tab"]';

function visible(el: Element): boolean {
  const r = el.getBoundingClientRect();
  if (r.width < 2 || r.height < 2) return false;
  const s = getComputedStyle(el);
  return s.visibility !== "hidden" && s.display !== "none" && !el.closest("[inert], [aria-hidden='true']");
}

/** Where focus may go: inside the topmost open dialog or menu, if there is one. */
function scope(): ParentNode {
  const layers = [...document.querySelectorAll('[aria-modal="true"], [role="dialog"], [role="menu"], [role="listbox"]')].filter(visible);
  return layers.at(-1) ?? document;
}

function candidates(): HTMLElement[] {
  return [...scope().querySelectorAll<HTMLElement>(FOCUSABLE)].filter(visible);
}

/** The nearest control from `from` in direction `dir`: straight ahead counts more than off to
 * the side, and anything overlapping on the cross axis wins over anything that doesn't. */
function nearest(from: DOMRect, dir: Dir, list: HTMLElement[], self: Element | null): HTMLElement | null {
  const cx = from.left + from.width / 2;
  const cy = from.top + from.height / 2;
  let best: HTMLElement | null = null;
  let bestScore = Infinity;
  for (const el of list) {
    if (el === self || el.contains(self) || self?.contains(el)) continue;
    const r = el.getBoundingClientRect();
    const ex = r.left + r.width / 2;
    const ey = r.top + r.height / 2;
    let main: number, cross: number, overlap: boolean;
    if (dir === "left" || dir === "right") {
      main = dir === "right" ? r.left - from.right : from.left - r.right;
      if (dir === "right" ? ex <= cx + 1 : ex >= cx - 1) continue;
      cross = Math.abs(ey - cy);
      overlap = r.bottom > from.top && r.top < from.bottom;
    } else {
      main = dir === "down" ? r.top - from.bottom : from.top - r.bottom;
      if (dir === "down" ? ey <= cy + 1 : ey >= cy - 1) continue;
      cross = Math.abs(ex - cx);
      overlap = r.right > from.left && r.left < from.right;
    }
    const score = Math.max(main, 0) + cross * (overlap ? 0.5 : 2.5) + (overlap ? 0 : 400);
    if (score < bestScore) {
      bestScore = score;
      best = el;
    }
  }
  return best;
}

/** The control the controller is on. Marked with an attribute as well as focused: the ring
 * must not depend on the window having keyboard focus (gamescope may not give it). */
let current: HTMLElement | null = null;

function focus(el: HTMLElement) {
  if (current) delete current.dataset.padFocus;
  current = el;
  el.dataset.padFocus = "";
  el.focus({ preventScroll: true });
  el.scrollIntoView({ block: "nearest", inline: "nearest" });
}

function move(dir: Dir) {
  const list = candidates();
  // Where the controller is: the marked control while it's still there, else the focused one.
  const active = current?.isConnected && list.includes(current) ? current : (document.activeElement as HTMLElement | null);
  const inScope = active && active !== document.body && list.includes(active);
  if (!inScope) {
    // Nothing focused yet (or focus is outside a menu that just opened): start at the top left.
    const first = list.sort((a, b) => {
      const ra = a.getBoundingClientRect();
      const rb = b.getBoundingClientRect();
      // Same row (within a few pixels): leftmost first.
      return Math.abs(ra.top - rb.top) > 12 ? ra.top - rb.top : ra.left - rb.left;
    })[0];
    if (first) focus(first);
    return;
  }
  // A slider takes left/right itself.
  if (active instanceof HTMLInputElement && active.type === "range" && (dir === "left" || dir === "right")) {
    if (dir === "left") active.stepDown();
    else active.stepUp();
    active.dispatchEvent(new Event("input", { bubbles: true }));
    active.dispatchEvent(new Event("change", { bubbles: true }));
    return;
  }
  const target = nearest(active.getBoundingClientRect(), dir, list, active);
  if (target) focus(target);
}

function press() {
  const el = current?.isConnected ? current : (document.activeElement as HTMLElement | null);
  if (!el || el === document.body) {
    move("down");
    return;
  }
  if (el instanceof HTMLInputElement && (el.type === "text" || el.type === "search")) {
    el.focus(); // brings up the system's on-screen keyboard, where there is one
    return;
  }
  if (el instanceof HTMLSelectElement) {
    // A closed <select> can't be opened from script: step through its options instead.
    el.selectedIndex = (el.selectedIndex + 1) % Math.max(el.options.length, 1);
    el.dispatchEvent(new Event("change", { bubbles: true }));
    return;
  }
  el.click();
}

function back() {
  const el = (current?.isConnected ? current : (document.activeElement as HTMLElement | null)) ?? document.body;
  const ev = new KeyboardEvent("keydown", { key: "Escape", code: "Escape", bubbles: true, cancelable: true });
  el.dispatchEvent(ev);
}

function scrollable(from: Element | null): Element {
  for (let el = from; el; el = el.parentElement) {
    const s = getComputedStyle(el);
    if ((s.overflowY === "auto" || s.overflowY === "scroll") && el.scrollHeight > el.clientHeight) return el;
  }
  return document.scrollingElement ?? document.body;
}

/** One press of `button` (a name from pad.rs). Exported for testing in a browser. */
export function handlePad(button: string, actions: PadActions) {
  switch (button) {
    case "up":
    case "down":
    case "left":
    case "right":
      move(button);
      break;
    case "a":
      press();
      break;
    case "b":
      back();
      break;
    case "x":
      actions.toggle();
      break;
    case "lb":
      actions.prev();
      break;
    case "rb":
      actions.next();
      break;
    case "lt":
      actions.seekBy(-10);
      break;
    case "rt":
      actions.seekBy(10);
      break;
  }
}

/** Listen to the controller. Returns the function that stops listening. */
export async function startGamepad(actions: PadActions): Promise<() => void> {
  const { listen } = await import("@tauri-apps/api/event");
  const root = document.documentElement;
  const padMode = (on: boolean) => {
    if (on) root.dataset.pad = "";
    else delete root.dataset.pad;
  };
  const offPad = await listen<string>("pad", ({ payload }) => {
    padMode(true);
    handlePad(payload, actions);
  });
  const offScroll = await listen<number>("pad-scroll", ({ payload }) => {
    padMode(true);
    const under = document.activeElement !== document.body ? document.activeElement : document.elementFromPoint(innerWidth / 2, innerHeight / 2);
    scrollable(under).scrollBy({ top: payload });
  });
  const pointer = () => padMode(false);
  window.addEventListener("pointermove", pointer, { passive: true });
  window.addEventListener("pointerdown", pointer, { passive: true });
  return () => {
    offPad();
    offScroll();
    window.removeEventListener("pointermove", pointer);
    window.removeEventListener("pointerdown", pointer);
  };
}
