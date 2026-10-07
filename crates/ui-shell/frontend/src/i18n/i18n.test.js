import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it } from "vitest";
import {
  FALLBACK_LOCALE,
  availableLocales,
  i18n,
  isLocaleTag,
  preferredLocale,
  registerMessages,
  t,
  tn,
} from "./i18n.svelte.js";

const SRC = fileURLToPath(new URL("..", import.meta.url));
const LOCALES = join(SRC, "i18n", "locales");

const catalogs = Object.fromEntries(
  readdirSync(LOCALES)
    .filter((file) => file.endsWith(".json"))
    .map((file) => [file.slice(0, -".json".length), JSON.parse(readFileSync(join(LOCALES, file), "utf8"))]),
);
const english = catalogs[FALLBACK_LOCALE];

const placeholders = (message) => [...message.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();
/** `watchlist.count.one` and `.other` are forms of `watchlist.count`. */
const PLURAL_FORM = /\.(zero|one|two|few|many|other)$/;

/**
 * Message ids the source names literally: arguments of `t(...)` and
 * `tn(...)`, and the `title`/`description` of registered screens and
 * layouts. Ids built at run time (`status.${status}`) are checked below.
 */
function idsUsedInSource() {
  const uses = [/\btn?\(\s*["']([^"']+)["']/g, /\b(?:title|description):\s*["']([a-z][\w-]*(?:\.[\w-]+)+)["']/g];
  const files = readdirSync(SRC, { recursive: true }).filter(
    (file) => /\.(js|svelte)$/.test(file) && !file.endsWith(".test.js"),
  );
  const ids = new Set();
  for (const file of files) {
    const source = readFileSync(join(SRC, file), "utf8");
    for (const use of uses) for (const [, id] of source.matchAll(use)) ids.add(id);
  }
  return [...ids];
}

afterEach(() => {
  i18n.locale = FALLBACK_LOCALE;
});

describe("catalogs", () => {
  it("English has every message id the interface uses", () => {
    const missing = idsUsedInSource().filter((id) => !(id in english) && !(`${id}.other` in english));

    expect(missing).toEqual([]);
  });

  it.each(Object.keys(catalogs))("%s only translates ids English has", (locale) => {
    const unknown = Object.keys(catalogs[locale]).filter(
      (id) => !(id in english) && !(PLURAL_FORM.test(id) && `${id.replace(PLURAL_FORM, "")}.other` in english),
    );

    expect(unknown).toEqual([]);
  });

  it.each(Object.keys(catalogs))("%s keeps the placeholders of each message", (locale) => {
    const mismatched = Object.entries(catalogs[locale])
      .filter(([id]) => id in english)
      .filter(([id, message]) => placeholders(message).join() !== placeholders(english[id]).join())
      .map(([id]) => id);

    expect(mismatched).toEqual([]);
  });

  it("every order status and side has a label", () => {
    const ids = ["submitted", "accepted", "partial", "filled", "cancelling", "cancelled", "rejected", "denied", "unknown"]
      .map((status) => `status.${status}`)
      .concat(["side.buy", "side.sell"]);

    expect(ids.filter((id) => !(id in english))).toEqual([]);
  });

  it("are named by valid language tags", () => {
    expect(Object.keys(catalogs).filter((locale) => !isLocaleTag(locale))).toEqual([]);
  });

  it("names each language in itself", () => {
    expect(availableLocales()).toContainEqual({ id: "en", name: "English" });
    expect(availableLocales()).toContainEqual({ id: "vi", name: "Tiếng Việt" });
  });
});

describe("lookup", () => {
  it("fills placeholders and leaves unknown ones visible", () => {
    expect(t("orders.cancelOrder", { id: 7 })).toBe("Cancel order 7");
    expect(t("orders.cancelOrder")).toBe("Cancel order {id}");
  });

  it("leaves a placeholder given no value empty, and ignores inherited names", () => {
    expect(t("ticket.denied", { reason: null })).toBe("Denied by risk: ");
    expect(t("ticket.denied", { reason: undefined })).toBe("Denied by risk: ");
    registerMessages("en", { "test.inherited": "{constructor}" });
    expect(t("test.inherited", {})).toBe("{constructor}");
  });

  it("refuses messages for something that is not a language tag", () => {
    expect(() => registerMessages("pt_BR", { "app.live": "ao vivo" })).toThrow(/language tag/);
  });

  it("falls back to English, then to the id itself", () => {
    registerMessages("xx", { "app.live": "xx-live" });
    i18n.locale = "xx";

    expect(t("app.live")).toBe("xx-live");
    expect(t("app.connecting")).toBe("connecting");
    expect(t("A plugin's plain title")).toBe("A plugin's plain title");
  });

  it("picks the plural form the language uses", () => {
    expect(tn("account.positions", 1)).toBe("1 position");
    expect(tn("account.positions", 3)).toBe("3 positions");

    i18n.locale = "vi";
    expect(tn("account.positions", 1)).toBe("1 vị thế");
  });

  it("prefers the language's own general form to an English specific one", () => {
    registerMessages("eo", { "test.items.other": "{count} eroj" });
    registerMessages("en", { "test.items.one": "{count} item", "test.items.other": "{count} items" });
    i18n.locale = "eo";

    expect(tn("test.items", 1)).toBe("1 eroj");
  });

  it("matches the system language by its primary tag", () => {
    expect(preferredLocale(["vi-VN", "en-US"])).toBe("vi");
    expect(preferredLocale(["fr-FR"])).toBe(FALLBACK_LOCALE);
    expect(preferredLocale([])).toBe(FALLBACK_LOCALE);
  });
});
