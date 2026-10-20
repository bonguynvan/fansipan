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

/**
 * What an order's note says: the venue's reason, or else why the order is
 * unusual: a command the venue has not answered, not yet
 * confirmed since a restart, or placed elsewhere and adopted.
 */
export function orderNote(order, t) {
  if (order.reason) return order.reason;
  if (order.unanswered) return t("order.unansweredReason");
  if (order.unconfirmed) return t("order.unconfirmedReason");
  if (order.external) return t("order.placedElsewhere");
  return "";
}

/** An order's status, with what is still unknown about it. */
export function statusText(order, t) {
  const status = t(`status.${order.status}`);
  if (order.unanswered) return t("status.unanswered", { status });
  if (order.unconfirmed) return t("status.unconfirmed", { status });
  return status;
}
