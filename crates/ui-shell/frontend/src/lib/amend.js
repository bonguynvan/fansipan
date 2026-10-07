// What the blotter's amend form lets through, from what was typed. Risk
// checks the new terms again in exact decimals and says why it refuses;
// these only keep the form from sending what cannot be an amendment.

import { isSendable } from "./ticket.js";

/** Whether the blotter offers to amend `order`: the engine says the order could change, on a venue that amends. */
export const canAmendOrder = (order, venueAmends) => Boolean(venueAmends && order?.amendable);

/**
 * Why the typed terms cannot be sent, or null when they can: "quantity" or
 * "price" when one is missing, "belowFilled" when the quantity is not above
 * what has filled, "unchanged" when they are the order's terms already.
 */
export function amendProblem(order, quantity, limitPrice) {
  if (!isSendable(quantity)) return "quantity";
  if (!isSendable(limitPrice)) return "price";
  if (Number(quantity) <= Number(order.filled_quantity)) return "belowFilled";
  const sameQuantity = Number(quantity) === Number(order.quantity);
  const samePrice = Number(limitPrice) === Number(order.limit_price);
  return sameQuantity && samePrice ? "unchanged" : null;
}

/**
 * The text to send for one term: the order's own decimal string while the
 * typed number still equals it, so an untouched term reaches Rust exactly
 * as the engine gave it; otherwise what was typed.
 */
export function amendTerm(typed, original) {
  if (typed === null || typed === undefined) return "";
  return Number(typed) === Number(original) ? original : String(typed);
}
