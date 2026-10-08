// Which tab strips move a frameless extra window (ADR 0013): the empty part
// of each docked strip, as a title bar would. A floating group's strip moves
// that group instead, unless nothing is docked and it is the only handle the
// window has.

/**
 * @template T
 * @param {T[]} strips every tab strip in the window
 * @param {(strip: T) => boolean} isFloating whether a strip belongs to a floating group
 * @returns {Set<T>} the strips that should move the window
 */
export function windowHandles(strips, isFloating) {
  const docked = strips.filter((strip) => !isFloating(strip));
  return new Set(docked.length ? docked : strips);
}
