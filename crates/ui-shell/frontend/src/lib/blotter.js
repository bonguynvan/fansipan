// Which of the session's orders a blotter shows, by its `view` parameter:
// the working ones to act on, the finished ones to look back on, or all.
// "Working" is the engine's word (an order's `open` flag), not the UI's.

export const VIEWS = ["working", "finished", "all"];

/** A blotter is for working from, so it opens on the working orders. */
export const DEFAULT_VIEW = "working";

/** The view a blotter's parameters ask for, or the default. */
export const viewOf = (params) => (VIEWS.includes(params?.view) ? params.view : DEFAULT_VIEW);

/** The orders `view` shows, in the order given. */
export function ordersIn(view, orders) {
  if (view === "all") return orders;
  const working = view === "working";
  return orders.filter((order) => Boolean(order.open) === working);
}

/** How many orders each view holds. */
export function viewCounts(orders) {
  const working = orders.filter((order) => order.open).length;
  return { working, finished: orders.length - working, all: orders.length };
}
