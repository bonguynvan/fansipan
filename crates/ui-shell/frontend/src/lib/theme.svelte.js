// The colour theme: dark, light, or the system's. The palette is
// tokens in app.css, dark under `:root` and light under
// `:root[data-theme="light"]`; this module sets that attribute. What cannot
// read CSS (the chart's canvas, dockview's base theme) reads
// `theme.resolved` instead.
//
// The theme is an app-wide setting (`ui.theme`), the same in every window.

import { sharedChoice } from "./sharedChoice.js";

/** The themes a trader can choose; "system" follows the operating system's. */
export const THEMES = ["dark", "light", "system"];
export const isTheme = (value) => THEMES.includes(value);

class Theme {
  /** The chosen theme: one of THEMES. */
  choice = $state("dark");
  /** The theme on screen now: "dark" or "light". */
  resolved = $state("dark");
}

export const theme = new Theme();

/** The theme a choice shows: the system's own for "system". */
export const resolveTheme = (choice, systemIsLight) =>
  choice === "system" ? (systemIsLight ? "light" : "dark") : choice;

/** The system's preference, as one query whose changes `startTheme` follows. */
const systemLight = globalThis.matchMedia?.("(prefers-color-scheme: light)") ?? null;

function show() {
  theme.resolved = resolveTheme(theme.choice, systemLight?.matches ?? false);
  document.documentElement.dataset.theme = theme.resolved;
}

function apply(choice) {
  theme.choice = choice;
  show();
}

const shared = sharedChoice({
  setting: "ui.theme",
  event: "ui-theme",
  isValid: isTheme,
  current: () => theme.choice,
  apply,
});

/** Sets the theme here, in every other window, and in the store. */
export const setTheme = (choice) => shared.choose(choice);

/** Applies the saved theme, follows other windows and the system's own; once per window. */
export function startTheme(options) {
  // The same listener on the same query is added once, so a second start adds nothing.
  systemLight?.addEventListener("change", show);
  return shared.start(options);
}
