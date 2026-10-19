// The built-in menus and the actions behind them (ADR 0017). They register
// through the calls a plugin uses (menus.js, hotkeys.svelte.js), so every
// command that is an action can be given a key in Keyboard shortcuts, and
// the menu shows it. The main window registers them: it alone has the menu
// bar, and a page command in an extra window would act on pages it does not
// show.

import { getCurrentWindow } from "@tauri-apps/api/window";
import { registerAction } from "./hotkeys.svelte.js";
import { registerMenu, registerMenuItem } from "./menus.js";
import { openSearch } from "./searchEvent.js";
import { TRADE, showSection } from "./sections.svelte.js";
import { THEMES, setTheme, theme } from "./theme.svelte.js";
import { market } from "./market.svelte.js";
import { TRADING_STATES, setTradingState } from "./tradingActions.js";
import { SCALES, setScale, ui } from "./ui.svelte.js";
import { availableLocales, i18n, setLocale, t } from "../i18n/i18n.svelte.js";
import { activePage } from "../workspace/pages.js";
import { listScreens } from "../workspace/registry.js";
import {
  addScreen,
  closePageTab,
  copyPage,
  newDesktop,
  newPage,
  popOut,
  workspace,
} from "../workspace/workspace.svelte.js";

const percent = (scale) => `${Math.round(scale * 100)}%`;

const isField = (element) => element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement;

/** Cut, copy, or select all in the field that had the keyboard; the browser does the work. */
const inField = (command) => () => {
  if (isField(document.activeElement)) document.execCommand(command);
};

/**
 * Pastes into the field that had the keyboard; Ctrl+V does it without
 * asking to read the clipboard. A field with no text selection to paste
 * into (a number field) takes the clipboard as its whole value.
 */
async function paste() {
  const field = document.activeElement;
  if (!isField(field)) return null;
  let text;
  try {
    text = await navigator.clipboard.readText();
  } catch (err) {
    return { id: "menu.pasteFailed", vars: { error: String(err) } };
  }
  if (field.selectionStart === null) field.value = text.trim();
  else field.setRangeText(text, field.selectionStart, field.selectionEnd ?? field.selectionStart, "end");
  field.dispatchEvent(new Event("input", { bubbles: true }));
  return null;
}

function closeThisPage() {
  if (workspace.pages) closePageTab(activePage(workspace.pages).id);
}

function popOutActive() {
  const panel = workspace.api?.activePanel;
  return panel ? popOut(panel) : { id: "menu.noPanel" };
}

async function toggleFullScreen() {
  const window = getCurrentWindow();
  await window.setFullscreen(!(await window.isFullscreen()));
}

/** Quitting is closing the main window: the shell saves every window, then exits. */
const exit = () => getCurrentWindow().close();

/**
 * Registers the built-in menus and the app's own actions. `showShortcuts`
 * and `showAbout` open the dialogs the main window owns.
 */
export function registerAppMenus({ showShortcuts, showAbout }) {
  registerAction({ id: "workspace.new-page", title: "hotkey.action.newPage", defaultKey: "Ctrl+T", run: () => newPage() });
  registerAction({ id: "workspace.copy-page", title: "hotkey.action.copyPage", run: copyPage });
  registerAction({ id: "workspace.close-page", title: "hotkey.action.closePage", defaultKey: "Ctrl+W", run: closeThisPage });
  registerAction({ id: "workspace.new-desktop", title: "hotkey.action.newDesktop", run: newDesktop });
  registerAction({ id: "workspace.pop-out", title: "hotkey.action.popOut", run: popOutActive });
  registerAction({ id: "app.full-screen", title: "hotkey.action.fullScreen", defaultKey: "F11", run: toggleFullScreen });
  registerAction({ id: "app.shortcuts", title: "hotkey.action.shortcuts", run: showShortcuts });
  registerAction({ id: "app.about", title: "hotkey.action.about", run: showAbout });
  registerAction({ id: "app.search", title: "hotkey.action.search", defaultKey: "Ctrl+K", run: openSearch });
  registerAction({ id: "app.exit", title: "hotkey.action.exit", run: exit });

  registerMenu({ id: "file", title: "menu.file", order: 10 });
  registerMenu({ id: "edit", title: "menu.edit", order: 20 });
  registerMenu({ id: "view", title: "menu.view", order: 30 });
  registerMenu({ id: "trade", title: "menu.trade", order: 40 });
  registerMenu({ id: "window", title: "menu.window", order: 80 });
  registerMenu({ id: "help", title: "menu.help", order: 90 });

  for (const action of ["workspace.new-page", "workspace.copy-page", "workspace.close-page"]) {
    registerMenuItem("file", { group: "page", action });
  }
  registerMenuItem("file", { group: "desktop", action: "workspace.new-desktop" });
  registerMenuItem("file", { group: "app", action: "app.exit" });

  registerMenuItem("edit", { title: "menu.cut", hint: "Ctrl+X", run: inField("cut") });
  registerMenuItem("edit", { title: "menu.copy", hint: "Ctrl+C", run: inField("copy") });
  registerMenuItem("edit", { title: "menu.paste", hint: "Ctrl+V", run: paste });
  registerMenuItem("edit", { group: "select", title: "menu.selectAll", hint: "Ctrl+A", run: inField("selectAll") });
  registerMenuItem("edit", { group: "find", action: "app.search" });

  registerMenuItem("view", {
    group: "panels",
    title: "workspace.addPanel",
    choices: () =>
      listScreens().map((screen) => ({
        id: screen.id,
        label: t(screen.title),
        pick: () => {
          showSection(TRADE);
          addScreen(screen.id);
        },
      })),
  });
  registerMenuItem("view", {
    group: "look",
    title: "menu.theme",
    choices: () =>
      THEMES.map((choice) => ({
        id: choice,
        label: t(`app.themeOption.${choice}`),
        checked: theme.choice === choice,
        pick: () => setTheme(choice),
      })),
  });
  registerMenuItem("view", {
    group: "look",
    title: "menu.textSize",
    choices: () =>
      SCALES.map((scale) => ({
        id: String(scale),
        label: percent(scale),
        checked: ui.scale === scale,
        pick: () => setScale(scale),
      })),
  });
  registerMenuItem("view", {
    group: "look",
    title: "menu.language",
    choices: () =>
      availableLocales().map((locale) => ({
        id: locale.id,
        label: locale.name,
        checked: i18n.locale === locale.id,
        pick: () => setLocale(locale.id),
      })),
  });
  registerMenuItem("view", { group: "window", action: "app.full-screen" });

  for (const action of ["ticket.buy", "ticket.sell", "ticket.focus"]) {
    registerMenuItem("trade", { group: "ticket", action });
  }
  for (const action of ["orders.cancel-symbol", "orders.cancel-all"]) {
    registerMenuItem("trade", { group: "cancel", action });
  }
  registerMenuItem("trade", { group: "switch", action: "trading.halt" });
  registerMenuItem("trade", {
    group: "switch",
    title: "menu.trading",
    choices: () =>
      TRADING_STATES.map((state) => ({
        id: state,
        label: t(`trading.state.${state}`),
        checked: market.trading === state,
        pick: () => setTradingState(state),
      })),
  });

  for (const action of [
    "workspace.next-panel",
    "workspace.previous-panel",
    "workspace.next-page",
    "workspace.previous-page",
  ]) {
    registerMenuItem("window", { group: "move", action });
  }
  registerMenuItem("window", { group: "panel", action: "workspace.pop-out" });

  registerMenuItem("help", { group: "keys", action: "app.shortcuts" });
  registerMenuItem("help", { group: "about", action: "app.about" });
}
