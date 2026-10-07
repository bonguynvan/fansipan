// Extra-window labels. The shell accepts `ws-` followed by 1-24 of a-z and
// 0-9 (workspace.rs `is_extra_window`); keep the two in step.

export const EXTRA_WINDOW_LABEL = /^ws-[a-z0-9]{1,24}$/;

/** A fresh extra-window label: base-36 time plus four random characters. */
export function newWindowLabel(now = Date.now(), random = Math.random()) {
  return `ws-${now.toString(36)}${random.toString(36).slice(2, 6)}`;
}
