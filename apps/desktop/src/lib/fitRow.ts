// A one-line row (the header) that has to fit whatever the language: when its content is wider
// than the row, add the given classes one by one until it fits, so CSS can drop the least needed
// parts in order. Each class stays on only while it is needed: a wider window or a shorter
// language brings everything back. Whatever still doesn't fit is left to CSS (ellipsis).
export function fitRow(node: HTMLElement, steps: string[]) {
  let frame = 0;

  // From the first child's edge to the last one's, against the room inside the padding
  // (scrollWidth misses content that only runs into the end padding).
  const overflows = () => {
    const kids = [...node.children].map((c) => c.getBoundingClientRect()).filter((r) => r.width > 0);
    if (!kids.length) return false;
    const used = Math.max(...kids.map((r) => r.right)) - Math.min(...kids.map((r) => r.left));
    const s = getComputedStyle(node);
    return used > node.clientWidth - parseFloat(s.paddingLeft) - parseFloat(s.paddingRight) + 0.5;
  };

  const fit = () => {
    cancelAnimationFrame(frame);
    // Measure at natural size: with "fit-measure" nothing in the row shrinks, so overflow shows.
    node.classList.add("fit-measure");
    let level = 0;
    const apply = () => steps.forEach((c, i) => node.classList.toggle(c, i < level));
    apply();
    while (level < steps.length && overflows()) {
      level++;
      apply();
    }
    node.classList.remove("fit-measure");
  };
  const later = () => {
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(fit);
  };

  // The window and the row resizing, and any text inside changing (language, device, state).
  const ro = new ResizeObserver(later);
  ro.observe(node);
  const mo = new MutationObserver(later);
  mo.observe(node, { childList: true, subtree: true, characterData: true });
  document.fonts?.ready.then(later);
  fit();

  return {
    destroy() {
      cancelAnimationFrame(frame);
      ro.disconnect();
      mo.disconnect();
    },
  };
}
