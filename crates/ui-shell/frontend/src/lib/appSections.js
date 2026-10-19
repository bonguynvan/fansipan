// The built-in sections (ADR 0017), registered through the call a plugin
// uses. Trade is the workspace; Settings and Help each have something to
// show. Inbox and History come with Phase 8, Money and Support from an
// adapter that offers them.

import HelpSection from "./HelpSection.svelte";
import SettingsSection from "./SettingsSection.svelte";
import { TRADE, registerSection } from "./sections.svelte.js";

// Icons in a 24-unit box, drawn as strokes.
const CANDLES = "M6 3v4M6 15v6M4 7h4v8H4zM18 3v3M18 14v7M16 6h4v8h-4zM12 5v5M12 17v3M10 10h4v7h-4z";
const GEAR =
  "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z";
const QUESTION = "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM9.1 9a3 3 0 0 1 5.8 1c0 2-3 3-3 3M12 17h.01";

export function registerAppSections() {
  registerSection({ id: TRADE, title: "section.trade", icon: CANDLES });
  registerSection({ id: "settings", title: "section.settings", icon: GEAR, component: SettingsSection, order: 80 });
  registerSection({ id: "help", title: "section.help", icon: QUESTION, component: HelpSection, order: 90 });
}
