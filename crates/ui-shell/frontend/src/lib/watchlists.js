// Named watchlists as plain data: `[{ id, name, symbols }]`, in the order
// the trader made them, each list's symbols in the order the trader put
// them. Every change returns new lists; nothing here mutates or saves.
// "All" is built in: every symbol the venue lists, never stored.

export const ALL = "all";
/** Longest list name kept; a name is a label in a narrow header. */
export const MAX_NAME = 40;

/**
 * The settings key a market's lists are kept under. Symbols belong to a
 * venue, so each market has its own lists; the id is made safe for a key
 * (a-z, 0-9, `.`, `-`, `_`), whatever an adapter calls its market.
 */
export const listsSetting = (marketId) =>
  `screen.watchlist.lists.${String(marketId || "default")
    .toLowerCase()
    .replace(/[^a-z0-9._-]/g, "-")}`;

/** A name as kept: spaces collapsed, trimmed, at most MAX_NAME characters. */
export const cleanName = (name) =>
  String(name ?? "")
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, MAX_NAME);

/** A symbol as kept: trimmed and upper-case. */
export const cleanSymbol = (symbol) =>
  String(symbol ?? "")
    .trim()
    .toUpperCase();

const unique = (symbols) => [...new Set(symbols)];

/**
 * Lists read back from the settings store, which may hold anything: only
 * well-formed lists, ids unique, names non-empty, each symbol once.
 */
export function normalizeLists(saved) {
  if (!Array.isArray(saved)) return [];
  const ids = new Set();
  return saved.flatMap((list) => {
    const name = cleanName(list?.name);
    if (typeof list?.id !== "string" || !list.id || list.id === ALL || ids.has(list.id) || !name) return [];
    ids.add(list.id);
    const symbols = Array.isArray(list.symbols)
      ? unique(list.symbols.filter((s) => typeof s === "string").map(cleanSymbol).filter(Boolean))
      : [];
    return [{ id: list.id, name, symbols }];
  });
}

const update = (lists, id, change) => lists.map((list) => (list.id === id ? change(list) : list));

export const withList = (lists, id, name) => [...lists, { id, name: cleanName(name), symbols: [] }];

/** Renamed; a name that cleans to nothing leaves the list as it was. */
export function renamed(lists, id, name) {
  const clean = cleanName(name);
  return clean ? update(lists, id, (list) => ({ ...list, name: clean })) : lists;
}

export const withoutList = (lists, id) => lists.filter((list) => list.id !== id);

export const withSymbol = (lists, id, symbol) =>
  update(lists, id, (list) => ({ ...list, symbols: unique([...list.symbols, cleanSymbol(symbol)]) }));

export const withoutSymbol = (lists, id, symbol) =>
  update(lists, id, (list) => ({ ...list, symbols: list.symbols.filter((s) => s !== symbol) }));

/** The list's symbol at `from` moved to `to`; out-of-range indices change nothing. */
export function moved(lists, id, from, to) {
  return update(lists, id, (list) => {
    const n = list.symbols.length;
    if (from === to || from < 0 || to < 0 || from >= n || to >= n) return list;
    const symbols = [...list.symbols];
    const [symbol] = symbols.splice(from, 1);
    symbols.splice(to, 0, symbol);
    return { ...list, symbols };
  });
}

/** The first of `nameFor(1)`, `nameFor(2)`, … no list has yet. */
export function nextListName(lists, nameFor) {
  const taken = new Set(lists.map((list) => list.name));
  let n = lists.length + 1;
  while (taken.has(nameFor(n))) n += 1;
  return nameFor(n);
}

/** The symbols a list shows: a named list's own, or every listed symbol for "All" or a list that is gone. */
export function symbolsOf(listId, lists, allSymbols) {
  return lists.find((list) => list.id === listId)?.symbols ?? allSymbols;
}

/**
 * Why `symbol` cannot join `list`, or null when it can: "empty",
 * "notListed" (the venue does not list it), or "already".
 */
export function addProblem(list, symbol, listed) {
  const clean = cleanSymbol(symbol);
  if (!clean) return "empty";
  if (!listed.has(clean)) return "notListed";
  return list.symbols.includes(clean) ? "already" : null;
}
