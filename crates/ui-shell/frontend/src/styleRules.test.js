// The UI rules a machine can check (docs/ui-rules.md, ADR 0014): sizes in
// every component's styles and in app.css are rem or relative, never
// pixels, except for hairlines and shadows; font sizes come from the type
// scale.

import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const SRC = fileURLToPath(new URL(".", import.meta.url));

/** Properties whose pixel values are hairlines or shadows, which stay crisp at any size. */
const PIXELS_ALLOWED = /^(border(-[a-z]+)*|outline(-[a-z]+)*|box-shadow|text-shadow)$/;
const PIXEL = /(?<![\w.-])\d*\.?\d+px\b/;
const FONT_TOKEN = /var\(--text-(2xs|xs|sm|md|lg|xl|2xl)\)/;

function styleSources() {
  const files = readdirSync(SRC, { recursive: true }).filter((f) => f.endsWith(".svelte") || f.endsWith(".css"));
  return files.map((file) => {
    const text = readFileSync(join(SRC, file), "utf8");
    if (file.endsWith(".css")) return { file, css: text, offset: 0 };
    const start = text.indexOf("<style>");
    if (start === -1) return { file, css: "", offset: 0 };
    const end = text.indexOf("</style>", start);
    return { file, css: text.slice(start, end), offset: text.slice(0, start).split("\n").length - 1 };
  });
}

/** Every `property: value` declaration, with its line. */
function declarations({ file, css, offset }) {
  const found = [];
  const withoutComments = css.replace(/\/\*[\s\S]*?\*\//g, (c) => c.replace(/[^\n]/g, " "));
  withoutComments.split("\n").forEach((line, i) => {
    for (const match of line.matchAll(/(?<![\w-])((?:--)?[a-z][a-z0-9-]*)\s*:\s*([^;{}]+)/gi)) {
      found.push({ where: `${file}:${offset + i + 1}`, property: match[1].toLowerCase(), value: match[2].trim() });
    }
  });
  return found;
}

const all = styleSources().flatMap(declarations);

describe("UI rules", () => {
  it("finds the styles it checks", () => {
    expect(all.length).toBeGreaterThan(200);
  });

  it("sizes nothing in pixels but hairlines and shadows", () => {
    const pixels = all
      .filter((d) => PIXEL.test(d.value) && !PIXELS_ALLOWED.test(d.property))
      .map((d) => `${d.where}  ${d.property}: ${d.value}`);

    expect(pixels).toEqual([]);
  });

  it("takes every font size from the type scale", () => {
    const off = all
      .filter((d) => d.property === "font-size" || d.property === "font")
      // The root's own size is the trader's text size, the one dial the scale hangs from.
      .filter((d) => !FONT_TOKEN.test(d.value) && !d.value.includes("var(--ui-scale"))
      .filter((d) => !/^(inherit|unset|initial)$/.test(d.value))
      .map((d) => `${d.where}  ${d.property}: ${d.value}`);

    expect(off).toEqual([]);
  });
});
