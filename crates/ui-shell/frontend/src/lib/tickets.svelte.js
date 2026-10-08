// The order tickets open in this window, so a key can reach one: the
// ticket the trader used last, else the first one opened. A ticket hands
// over a small handle, not itself: `{ prepare(side), symbol() }`.

const open = [];
let last = null;

/** Adds a ticket; returns the function that removes it again. */
export function registerTicket(handle) {
  open.push(handle);
  return () => {
    const index = open.indexOf(handle);
    if (index !== -1) open.splice(index, 1);
    if (last === handle) last = null;
  };
}

/** Marks `handle` as the ticket the trader is using. */
export function useTicket(handle) {
  last = handle;
}

/** The ticket a key should reach, or null when this window has none. */
export function activeTicket() {
  return last ?? open[0] ?? null;
}
