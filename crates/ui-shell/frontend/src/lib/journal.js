// What the trader is told about the journal when the terminal starts
// (ADR 0023): one notice, the most pressing first. An error, then a damaged
// tail set aside, then orders the venue cannot confirm, then what came back.

/** The notice for the snapshot's journal report, or null when there is nothing to say. */
export function journalNotice(journal) {
  if (!journal) return null;
  if (journal.error) return { id: "journal.failed", vars: { error: journal.error } };
  const unanswered = journal.unconfirmed && !journal.venue_asked;
  if (journal.damaged) {
    return unanswered
      ? { id: "journal.damagedUnconfirmed", vars: { path: journal.damaged, unconfirmed: journal.unconfirmed } }
      : { id: "journal.damaged", vars: { path: journal.damaged } };
  }
  const orders = journal.restored_orders ?? 0;
  const held = journal.restored_conditionals ?? 0;
  if (!orders && !held) return null;
  if (unanswered) {
    return { id: "journal.restoredUnconfirmed", vars: { orders, held, unconfirmed: journal.unconfirmed } };
  }
  return { id: "journal.restored", vars: { orders, held } };
}
