// What the order ticket shows and sends, from what was typed. A field
// nobody has typed in shows a default; a field typed empty stays empty, so
// clearing it to type a new number does not bring the default back under
// the cursor.

/** A field nobody has typed in yet, as distinct from one typed empty (`null`). */
export const UNTYPED = undefined;

/** What was typed, or `fallback` while nothing has been. */
export const typedOr = (typed, fallback) => (typed === UNTYPED ? fallback : typed);

/** Whether a field holds a number above zero the ticket can send. */
export const isSendable = (value) => value !== null && value !== UNTYPED && value !== "" && Number(value) > 0;
