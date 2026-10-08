// How wide a screen is, as marks its styles can select on (ADR 0014,
// docs/ui-rules.md). A screen gets a panel the trader docks, floats, and
// resizes, so it adapts to that, not to the window. CSS container queries
// would do this, but a size container captures `position: fixed`
// descendants, and the grids' menus are fixed.

import { ui } from "./ui.svelte.js";

/** The widths, in rem, at which a screen's arrangement changes. */
export const WIDTHS = { narrow: 20, compact: 30, wide: 48 };

/** Which marks a screen `widthPx` wide carries when 1rem is `rootPx`. Compact includes narrow. */
export function sizeMarks(widthPx, rootPx) {
  return {
    narrow: widthPx < WIDTHS.narrow * rootPx,
    compact: widthPx < WIDTHS.compact * rootPx,
    wide: widthPx >= WIDTHS.wide * rootPx,
  };
}

/**
 * Action: marks `node` with `data-narrow`, `data-compact`, and `data-wide`
 * as its width crosses WIDTHS, and again when the text size changes.
 */
export function panelSize(node) {
  let width = node.getBoundingClientRect().width;

  function mark() {
    for (const [name, on] of Object.entries(sizeMarks(width, ui.rootPx))) {
      node.toggleAttribute(`data-${name}`, on);
    }
  }

  const observer = new ResizeObserver(([entry]) => {
    width = entry.contentRect.width;
    mark();
  });
  observer.observe(node);

  $effect(() => {
    ui.rootPx;
    mark();
  });

  return {
    destroy() {
      observer.disconnect();
    },
  };
}
