// Sizes that follow the trader's text size.
// CSS sizes in rem, so the root font size is the one dial. Code that must
// hand a library pixels (a grid's row height, a column's width) uses
// `px()`, which scales a size designed at 100% the same way.
//
// The text size is an app-wide setting (`ui.scale`), the same in every
// window.

import { sharedChoice } from "./sharedChoice.js";

/** The text sizes a trader can choose, as fractions of the default. */
export const SCALES = [0.9, 1, 1.15, 1.3];
/** The root font size, in pixels, at 100%: the browser's default. */
export const BASE_ROOT_PX = 16;

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

const textSize = sharedChoice({
  setting: "ui.scale",
  event: "ui-scale",
  isValid: isScale,
  current: () => ui.scale,
  apply,
});

/** Sets the text size here, in every other window, and in the store. */
export const setScale = (scale) => textSize.choose(scale);

/** Applies the saved text size and follows other windows' changes; once per window. */
export const startUiScale = (options) => textSize.start(options);
