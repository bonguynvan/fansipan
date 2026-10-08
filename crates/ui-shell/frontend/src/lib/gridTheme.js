// The theme every grid uses: bo-grid's TradeCanvas palette, sized in the
// app's tokens so cells, headers, and padding follow the trader's text size
// (ADR 0014). Row heights, which bo-grid takes as numbers, come from `px()`.

import { tradecanvasTheme } from "bo-grid";

export const gridTheme = {
  ...tradecanvasTheme,
  fontSize: "var(--text-md)",
  headerH: "1.75rem",
  cellPad: "0.5rem",
  radius: "0.5rem",
};

/** bo-grid's default row height, in pixels at 100%. */
export const ROW_HEIGHT_PX = 36;
