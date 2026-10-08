// The theme every grid uses: bo-grid applies a theme as CSS custom
// properties, so each value names one of the app's tokens and the grids
// follow the theme (ADR 0015) and the trader's text size (ADR 0014) with
// nothing to recompute. Row heights, which bo-grid takes as numbers, come
// from `px()`.

export const gridTheme = {
  bg: "var(--panel)",
  headerBg: "var(--panel-raised)",
  rowA: "var(--panel)",
  rowB: "var(--panel)",
  rowHover: "var(--panel-raised)",
  text: "var(--text)",
  textDim: "var(--text-dim)",
  border: "var(--rule)",
  up: "var(--up)",
  down: "var(--down)",
  amber: "var(--accent)",
  selFill: "color-mix(in srgb, var(--accent) 14%, transparent)",
  selBorder: "var(--accent)",
  scheme: "var(--scheme)",
  fontSize: "var(--text-md)",
  headerH: "1.75rem",
  cellPad: "0.5rem",
  radius: "0.5rem",
};

/** bo-grid's default row height, in pixels at 100%. */
export const ROW_HEIGHT_PX = 36;
