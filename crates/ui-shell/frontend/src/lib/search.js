// Search across what the trader can do and see: commands (every
// registered action, and every choice a menu offers, such as a screen to
// add or a theme) and symbols. Matching ignores case and accents, so
// "dat lenh" finds "Đặt lệnh" and "aapl" finds AAPL.

/** At most this many results are shown. */
export const MAX_RESULTS = 12;

/** Lower case, accents dropped (đ to d too), so a trader can type without them. */
export function fold(text) {
  return String(text ?? "")
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .replace(/đ/g, "d")
    .replace(/Đ/g, "D")
    .toLowerCase();
}

/**
 * How well `text` matches `query`, both folded: 0 the whole text, 1 its
 * start, 2 the start of a word in it, 3 anywhere; null not at all.
 */
export function matchScore(text, query) {
  if (!query) return null;
  if (text === query) return 0;
  if (text.startsWith(query)) return 1;
  const at = text.indexOf(query);
  if (at < 0) return null;
  return /[\s\p{P}]/u.test(text[at - 1]) ? 2 : 3;
}

/**
 * The entries that match `query`, best first: by how well their label (or,
 * less well, their detail) matches, then symbols before commands for an
 * equal match, then by label. An entry is `{ kind: "command" | "symbol",
 * label, detail?, run }`.
 */
export function search(query, entries, limit = MAX_RESULTS) {
  const wanted = fold(query).trim();
  if (!wanted) return [];
  const scored = [];
  for (const entry of entries) {
    const byLabel = matchScore(fold(entry.label), wanted);
    const byDetail = matchScore(fold(entry.detail), wanted);
    const score = byLabel ?? (byDetail === null ? null : byDetail + 4);
    if (score !== null) scored.push({ entry, score });
  }
  const kindRank = (entry) => (entry.kind === "symbol" ? 0 : 1);
  return scored
    .sort(
      (a, b) =>
        a.score - b.score || kindRank(a.entry) - kindRank(b.entry) || a.entry.label.localeCompare(b.entry.label),
    )
    .slice(0, limit)
    .map(({ entry }) => entry);
}

/**
 * Every command the menus and actions offer, as search entries: each
 * action once (with its key), and each choice in a menu's list as
 * "List: choice". Commands that act on the focused field (Cut, Paste) are
 * left out: from the search box, they would act on the search box.
 */
export function commandEntries({ actions, menus, translate, keyOf, runAction }) {
  const fromActions = actions.map((action) => ({
    kind: "command",
    label: translate(action.title),
    key: keyOf(action.id),
    run: () => runAction(action.id),
  }));
  const fromChoices = menus.flatMap((menu) =>
    menu.groups.flat().flatMap((item) =>
      item.kind === "choices"
        ? item.choices().map((choice) => ({
            kind: "command",
            label: `${translate(item.title)}: ${choice.label}`,
            run: choice.pick,
          }))
        : [],
    ),
  );
  return [...fromActions, ...fromChoices];
}

/** Every listed symbol as a search entry, with its name as the detail. */
export function symbolEntries(symbols, instruments, pick) {
  return symbols.map((symbol) => ({
    kind: "symbol",
    label: symbol,
    detail: instruments[symbol]?.name ?? "",
    run: () => pick(symbol),
  }));
}
