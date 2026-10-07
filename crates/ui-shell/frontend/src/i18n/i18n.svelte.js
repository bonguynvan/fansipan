// Every word the interface shows comes from a message catalog: one JSON
// file per locale in ./locales, mapping message ids to text. Components
// hold ids, never display text, so adding a language is adding a file.
//
// A message a catalog lacks falls back to English, and one English lacks
// shows its id: a gap is visible, never blank. Plugins add or override
// messages with `registerMessages`.
//
// The chosen locale is an app-wide setting, the same in every window.

import { emit, listen } from "@tauri-apps/api/event";
import { debouncedSetter, getSetting } from "../lib/settings.js";

export const FALLBACK_LOCALE = "en";
const SETTING = "ui.locale";
const EVENT = "locale-change";
// A language is chosen rarely, and a window opened right after should
// already read the new choice.
const SAVE_DELAY_MS = 0;
const PLACEHOLDER = /\{(\w+)\}/g;

/** Plural rules and number formats depend on the id, so it must be a BCP 47 tag: `pt-BR`, not `pt_BR`. */
export function isLocaleTag(id) {
  try {
    return Intl.getCanonicalLocales(id).length === 1;
  } catch {
    return false;
  }
}

const files = import.meta.glob("./locales/*.json", { eager: true, import: "default" });
const catalogs = new Map(
  Object.entries(files)
    .map(([path, messages]) => [path.slice(path.lastIndexOf("/") + 1, -".json".length), messages])
    .filter(([id]) => isLocaleTag(id)),
);

class I18n {
  locale = $state(FALLBACK_LOCALE);
  /** Bumped by `registerMessages`, so text already on screen picks up new messages. */
  revision = $state(0);
}

export const i18n = new I18n();

const save = debouncedSetter(SETTING, SAVE_DELAY_MS);
/** Set once the language has been chosen in this session, here or in another window. */
let chosen = false;

/** Adds messages to a locale's catalog (creating it), overriding ids it already has. */
export function registerMessages(locale, messages) {
  if (!isLocaleTag(locale)) throw new Error(`"${locale}" is not a BCP 47 language tag`);
  catalogs.set(locale, { ...(catalogs.get(locale) ?? {}), ...messages });
  i18n.revision += 1;
}

/** Locales with a catalog, each with its name in its own language. */
export function availableLocales() {
  return [...catalogs.keys()].sort().map((id) => ({ id, name: catalogs.get(id)["locale.name"] ?? id }));
}

function messageIn(locale, id) {
  // Read so that text depending on the catalogs re-renders when one grows.
  void i18n.revision;
  return catalogs.get(locale)?.[id];
}

function lookup(id) {
  return messageIn(i18n.locale, id) ?? messageIn(FALLBACK_LOCALE, id);
}

/** A placeholder with no value given stays visible; one given as null or undefined is left empty. */
function fill(message, vars) {
  if (!vars) return message;
  return message.replace(PLACEHOLDER, (match, name) => {
    if (!Object.hasOwn(vars, name)) return match;
    return vars[name] === null || vars[name] === undefined ? "" : String(vars[name]);
  });
}

/**
 * The message `id` in the current locale, with `{name}` placeholders
 * filled from `vars`. Text that is not a known id (a plugin's literal
 * screen title) comes back unchanged.
 */
export function t(id, vars) {
  return fill(lookup(id) ?? id, vars);
}

/**
 * A message that depends on a count: `${id}.one`, `${id}.few`, ... as the
 * locale's plural rules pick, else `${id}.other`. The locale's own forms
 * come before English's. `{count}` is filled in.
 */
export function tn(id, count, vars = {}) {
  const form = new Intl.PluralRules(i18n.locale).select(count);
  const message =
    messageIn(i18n.locale, `${id}.${form}`) ??
    messageIn(i18n.locale, `${id}.other`) ??
    messageIn(FALLBACK_LOCALE, `${id}.${form}`) ??
    messageIn(FALLBACK_LOCALE, `${id}.other`) ??
    id;
  return fill(message, { count, ...vars });
}

/** The supported locale closest to the system's preferences. */
export function preferredLocale(languages = globalThis.navigator?.languages ?? []) {
  for (const tag of languages) {
    if (catalogs.has(tag)) return tag;
    const primary = tag.split("-")[0];
    if (catalogs.has(primary)) return primary;
  }
  return FALLBACK_LOCALE;
}

function apply(locale) {
  if (!catalogs.has(locale) || i18n.locale === locale) return false;
  i18n.locale = locale;
  if (globalThis.document) document.documentElement.lang = locale;
  return true;
}

/** Switches this window and every other one, and remembers the choice. */
export function setLocale(locale) {
  if (!apply(locale)) return;
  chosen = true;
  save(locale);
  emit(EVENT, locale);
}

/** Writes a pending choice now; see the shell's quit sequence. */
export function flushLocale() {
  return save.flush();
}

let started = null;

/** Applies the saved locale (or the system's) and follows other windows; once per window. */
export function startLocale() {
  started ??= (async () => {
    await listen(EVENT, ({ payload }) => {
      if (apply(payload)) chosen = true;
    });
    let saved = null;
    try {
      saved = await getSetting(SETTING);
    } catch {
      // An unreadable setting is not fatal: the system language still applies.
    }
    // A choice made while the setting was being read is newer than it.
    if (chosen) return;
    apply(typeof saved === "string" && catalogs.has(saved) ? saved : preferredLocale());
  })();
  return started;
}
