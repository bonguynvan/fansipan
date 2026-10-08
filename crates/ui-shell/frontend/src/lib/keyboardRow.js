// Shows which row the keyboard is on in a grid that draws no cell
// selection. bo-grid moves its focus cell for the arrow, Home, End, and
// page keys, and names it in `aria-activedescendant`, but paints it only
// when cell selection is on, which display grids turn off. This marks that
// cell's row (`data-keyboard-row`) while the grid has keyboard focus, for
// app.css to outline. Put it on the element around a `Grid`.

const MARK = "data-keyboard-row";

/** @param {HTMLElement} node */
export function keyboardRow(node) {
  let marked = null;

  const grid = () => node.querySelector('[role="grid"]');
  const unmark = () => {
    marked?.removeAttribute(MARK);
    marked = null;
  };
  const mark = () => {
    unmark();
    const focused = grid();
    // Keyboard focus only: a click on a row already shows it.
    if (!focused?.matches(":focus-visible")) return;
    const id = focused.getAttribute("aria-activedescendant");
    const row = id ? document.getElementById(id)?.closest('[role="row"]') : null;
    if (!row) return;
    row.setAttribute(MARK, "");
    marked = row;
  };

  // The focus cell moves (the attribute changes, on a grid that may mount
  // after this element); rows are recycled as the grid scrolls, so a
  // scroll marks again whichever row now shows the cell.
  const moves = new MutationObserver(mark);
  moves.observe(node, { subtree: true, attributes: true, attributeFilter: ["aria-activedescendant"] });
  node.addEventListener("focusin", mark);
  node.addEventListener("focusout", unmark);
  node.addEventListener("scroll", mark, { capture: true, passive: true });

  return {
    destroy() {
      moves.disconnect();
      node.removeEventListener("focusin", mark);
      node.removeEventListener("focusout", unmark);
      node.removeEventListener("scroll", mark, { capture: true });
      unmark();
    },
  };
}
