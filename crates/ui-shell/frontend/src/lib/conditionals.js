// The pieces of the conditional-orders screen that are not markup: what it
// sends the shell, whether a form is complete, and how a held order reads
// (ADR 0020). The shell parses the numbers exactly and says what is wrong.

/** The triggers the shell takes, in the order the screen offers them. */
export const TRIGGERS = ["at_or_below", "at_or_above", "trailing"];

const blank = (value) => value === null || value === undefined || String(value).trim() === "";

/** Whether a form has what its trigger and its order need. */
export function complete(form) {
  if (!form.symbol || blank(form.quantity)) return false;
  if (form.trigger === "trailing" ? blank(form.offset) : blank(form.price)) return false;
  return form.sendAs !== "limit" || !blank(form.limitPrice);
}

/** The `place_conditional` arguments for a form; fields its trigger does not use are left out. */
export function placeArgs(form) {
  const trailing = form.trigger === "trailing";
  return {
    symbol: form.symbol,
    side: form.side,
    quantity: String(form.quantity),
    trigger: form.trigger,
    price: trailing ? null : String(form.price),
    offset: trailing ? String(form.offset) : null,
    percent: trailing && Boolean(form.percent),
    limitPrice: form.sendAs === "limit" ? String(form.limitPrice) : null,
  };
}

/** A held order's condition as a message id and its values. */
export function conditionWords(conditional) {
  if (conditional.trigger === "trailing") {
    return {
      id: conditional.percent ? "conditional.when.trailingPercent" : "conditional.when.trailing",
      vars: { offset: conditional.offset },
    };
  }
  return { id: `conditional.when.${conditional.trigger}`, vars: { price: conditional.price } };
}

/** Waiting ones first, then the rest; newest first within each. */
export function inOrder(conditionals) {
  const waiting = (c) => (c.status === "waiting" ? 0 : 1);
  return [...conditionals].sort((a, b) => waiting(a) - waiting(b) || b.id - a.id);
}

/** Conditionals by id, with `changed` laid over `held`. */
export function withConditionals(held, changed = []) {
  if (!changed.length) return held;
  return { ...held, ...Object.fromEntries(changed.map((c) => [c.id, c])) };
}
