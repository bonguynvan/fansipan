// The UI rules a machine can check (docs/ui-rules.md, ADR 0014, ADR 0015):
// sizes in every component's styles and in app.css are rem or relative,
// never pixels, except for hairline borders and outlines and for shadows;
// font sizes come from the type scale; colours come from the palette's
// tokens, which only app.css defines.

import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const SRC = fileURLToPath(new URL(".", import.meta.url));

/** Hairlines: a border or outline's width, at most HAIRLINE_MAX_PX. */
const HAIRLINE = /^(border|border-(top|right|bottom|left)|border(-(top|right|bottom|left))?-width|outline|outline-width|outline-offset)$/;
const HAIRLINE_MAX_PX = 3;
/** Shadows keep their pixel offsets and blur: depth, not size. So do the palette's shadow tokens. */
const SHADOW = /^(box-shadow|text-shadow|--shadow-[a-z-]+)$/;
const PIXEL = /(?<![\w.-])(\d*\.?\d+)px\b/g;
const FONT_TOKEN = /var\(--text-(2xs|xs|sm|md|lg|xl|2xl)\)/;
/** A colour written out, rather than taken from a token: a hex value, a colour function, or a basic colour keyword. */
const COLOUR =
  /#[0-9a-f]{3,8}\b|\b(rgba?|hsla?|hwb|lab|lch|oklab|oklch)\(|(?<![\w-])(aqua|black|blue|fuchsia|gray|grey|green|lime|maroon|navy|olive|orange|purple|red|silver|teal|white|yellow)(?![\w-])/i;
/** The one file that defines the palette. */
const PALETTE_FILE = "app.css";

/** Every style block of every component (any attributes, any number), and app.css. */
function styleSources() {
  const files = readdirSync(SRC, { recursive: true }).filter((f) => f.endsWith(".svelte") || f.endsWith(".css"));
  return files.flatMap((file) => {
    const text = readFileSync(join(SRC, file), "utf8");
    if (file.endsWith(".css")) return [{ file, css: text, offset: 0 }];
    return [...text.matchAll(/<style[^>]*>([\s\S]*?)<\/style>/g)].map((m) => {
      const start = m.index + m[0].indexOf(m[1]);
      return { file, css: m[1], offset: text.slice(0, start).split("\n").length - 1 };
    });
  });
}

/** Every `property: value` declaration, however it wraps across lines, with the line it starts on. */
function declarations({ file, css, offset }) {
  const text = css.replace(/\/\*[\s\S]*?\*\//g, (c) => c.replace(/[^\n]/g, " "));
  const found = [];
  let at = 0;
  for (const chunk of text.split(/[;{}]/)) {
    const match = /^\s*((?:--)?[a-z][a-z0-9-]*)\s*:\s*([\s\S]+?)\s*$/i.exec(chunk);
    if (match) {
      const line = offset + text.slice(0, at + chunk.indexOf(match[1])).split("\n").length;
      found.push({ where: `${file}:${line}`, property: match[1].toLowerCase(), value: match[2].replace(/\s+/g, " ") });
    }
    at += chunk.length + 1;
  }
  return found;
}

const pixelsIn = (value) => [...value.matchAll(PIXEL)].map((m) => Number(m[1]));

/** Whether a declaration's pixels break the rules. */
function breaksPixelRule({ property, value }) {
  const pixels = pixelsIn(value);
  if (pixels.length === 0 || SHADOW.test(property)) return false;
  if (HAIRLINE.test(property)) return pixels.some((n) => n > HAIRLINE_MAX_PX);
  return true;
}

/** Whether a declaration writes out a colour anywhere but in a palette token. */
function breaksColourRule({ file, property, value }) {
  if (!COLOUR.test(value)) return false;
  return !(file === PALETTE_FILE && property.startsWith("--"));
}

const all = styleSources().flatMap((source) => declarations(source).map((d) => ({ ...d, file: source.file })));

describe("UI rules", () => {
  it("finds the styles it checks", () => {
    expect(all.length).toBeGreaterThan(200);
  });

  it("sizes nothing in pixels but hairlines and shadows", () => {
    const pixels = all.filter(breaksPixelRule).map((d) => `${d.where}  ${d.property}: ${d.value}`);

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

  it("takes every colour from the palette's tokens", () => {
    const written = all.filter(breaksColourRule).map((d) => `${d.where}  ${d.property}: ${d.value}`);

    expect(written).toEqual([]);
  });
});

describe("the checker", () => {
  const check = (css) => declarations({ file: "x.svelte", css, offset: 0 }).filter(breaksPixelRule).map((d) => d.property);

  it("allows hairline borders and shadows", () => {
    expect(check("a { border: 1px solid red; border-left: 3px solid red; box-shadow: 0 10px 30px black; }")).toEqual([]);
  });

  it("catches sizes hiding in border properties", () => {
    expect(check("a { border-radius: 12px; border-spacing: 8px; border-top: 6px solid red; }")).toEqual([
      "border-radius",
      "border-spacing",
      "border-top",
    ]);
  });

  it("reads a declaration that wraps onto the next line", () => {
    expect(check("a {\n  grid-template-columns:\n    10px 1fr;\n}")).toEqual(["grid-template-columns"]);
  });

  it("allows a colour only where app.css defines a token", () => {
    const colours = (file, css) => declarations({ file, css, offset: 0 }).map((d) => ({ ...d, file })).filter(breaksColourRule).map((d) => d.property);

    expect(colours("app.css", ":root { --ink: #0b0f15; --shadow-menu: 0 1px 2px rgb(0 0 0 / 40%); }")).toEqual([]);
    expect(colours("app.css", "body { background: #fff; }")).toEqual(["background"]);
    expect(colours("x.svelte", "a { color: #abc; border-color: rgba(0, 0, 0, 0.5); outline-color: hsl(10 20% 30%); }")).toEqual([
      "color",
      "border-color",
      "outline-color",
    ]);
    expect(colours("x.svelte", "a { color: white; border: 1px solid red; }")).toEqual(["color", "border"]);
    expect(colours("x.svelte", "a { color: var(--text); background: color-mix(in srgb, var(--accent) 12%, transparent); }")).toEqual([]);
    expect(colours("x.svelte", "a { grid-template-areas: \"redline whitespace\"; animation-name: fade-white-out; }")).toEqual([]);
  });

  it("reads every style block, whatever its attributes", () => {
    const blocks = [...'<style lang="css">a { gap: 4px }</style><style>b { gap: 1rem }</style>'.matchAll(/<style[^>]*>([\s\S]*?)<\/style>/g)];

    expect(blocks.map((m) => check(m[1]))).toEqual([["gap"], []]);
  });
});
