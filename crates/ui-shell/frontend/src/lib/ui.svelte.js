// Sizes that follow the trader's text size (ADR 0014, docs/ui-rules.md).
// CSS sizes in rem, so the root font size is the one dial. Code that must
// hand a library pixels (a grid's row height, a column's width) uses
// `px()`, which scales a size designed at 100% the same way.
//
// The text size is an app-wide setting (`ui.scale`), the same in every
// window.

import { emit, listen } from "@tauri-apps/api/event";
import { debouncedSetter, getSetting } from "./settings.js";

/** The text sizes a trader can choose, as fractions of the default. */
export const SCALES = [0.9, 1, 1.15, 1.3];
/** The root font size, in pixels, at 100%: the browser's default. */
export const BASE_ROOT_PX = 16;
const SETTING = "ui.scale";
const EVENT = "ui-scale";
/** Ignores this window's own events, which Tauri delivers back late. */
const SOURCE = crypto.randomUUID();

class Ui {
  /** The chosen text size: one of SCALES. */
  scale = $state(1);
  /** The root font size in pixels now: 1rem. */
  rootPx = $state(BASE_ROOT_PX);
}

export const ui = new Ui();

export const isScale = (value) => SCALES.includes(value);

/** A size designed in pixels at 100%, in pixels at the current text size. Read it in `$derived` or markup. */
export const px = (designPx) => Math.round((designPx * ui.rootPx) / BASE_ROOT_PX);

/** Pixels in `rem` rems at the current text size. */
export const remToPx = (rem) => rem * ui.rootPx;

function apply(scale) {
  ui.scale = scale;
  const root = document.documentElement;
  root.style.setProperty("--ui-scale", String(scale));
  ui.rootPx = parseFloat(getComputedStyle(root).fontSize) || BASE_ROOT_PX * scale;
}

const save = debouncedSetter(SETTING, 0);
/** Shows the trader a size that did not reach the other windows; set by `startUiScale`. */
let report = () => {};
/** Set once a size is chosen here or arrives from another window: newer than the save. */
let chosen = false;

/** Sets the text size here, in every other window, and in the store. */
export function setScale(scale) {
  if (!isScale(scale) || scale === ui.scale) return;
  chosen = true;
  apply(scale);
  save(scale);
  emit(EVENT, { source: SOURCE, scale }).catch((err) => report(String(err)));
}

let started = null;

/** Applies the saved text size and follows other windows' changes; once per window. */
export function startUiScale({ onError } = {}) {
  started ??= (async () => {
    report = onError ?? report;
    apply(ui.scale);
    await listen(EVENT, ({ payload }) => {
      if (payload?.source === SOURCE || !isScale(payload?.scale)) return;
      chosen = true;
      apply(payload.scale);
    });
    const saved = await getSetting(SETTING);
    // A size chosen while the save was being read is the newer one.
    if (!chosen && isScale(saved)) apply(saved);
  })();
  return started;
}
