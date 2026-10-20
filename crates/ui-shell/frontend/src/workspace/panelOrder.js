// The order the keyboard walks a page's panels in (Ctrl+F6 by default):
// as the page is read, groups top to bottom and then left to right, and
// inside a group its tabs in order.

/**
 * Panels in walking order.
 * @template P
 * @param {{ top: number, left: number, panels: P[] }[]} groups
 * @returns {P[]}
 */
export function panelOrder(groups) {
  return [...groups]
    .sort((a, b) => a.top - b.top || a.left - b.left)
    .flatMap((group) => group.panels);
}

/**
 * The item `step` places from `current` in `order`, wrapping round; from
 * outside the order, the first going forward and the last going back.
 * @template T
 * @param {T[]} order
 * @param {T} current
 * @param {number} step
 * @returns {T | null}
 */
export function stepFrom(order, current, step) {
  const count = order.length;
  if (count === 0) return null;
  const at = order.indexOf(current);
  if (at < 0) return step > 0 ? order[0] : order[count - 1];
  return order[(((at + step) % count) + count) % count];
}
