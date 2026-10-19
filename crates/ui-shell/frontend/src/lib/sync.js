// Orders the shell's numbered batches against the startup snapshot.
//
// Batches can arrive before the snapshot's reply. Applying them first and
// the snapshot after would overwrite newer state with older; applying them
// all after the snapshot would replay older state over it. The snapshot
// carries the number of the last batch it already includes, so: hold
// batches until the snapshot is in, drop the ones it covers, and from then
// on ignore any batch older than the last one applied.

/** `apply(batch)` receives the snapshot (as a batch) and then each newer batch, in order. */
export function createSync(apply) {
  let synced = false;
  let lastSeq = 0;
  let early = [];

  function deliver(batch) {
    if (batch.seq <= lastSeq) return;
    lastSeq = batch.seq;
    apply(batch);
  }

  return {
    batch(batch) {
      if (synced) deliver(batch);
      else early = [...early, batch];
    },

    snapshot(snap) {
      apply({
        seq: snap.seq,
        quotes: snap.quotes,
        orders: snap.orders,
        account: snap.account,
        bars: [],
        instruments: snap.instruments ?? [],
        boards: snap.boards ?? [],
        connections: snap.connections ?? [],
        sessions: snap.sessions ?? [],
        statuses: snap.statuses ?? [],
        conditionals: snap.conditionals ?? [],
        prints: [],
      });
      lastSeq = snap.seq;
      synced = true;
      [...early].sort((a, b) => a.seq - b.seq).forEach(deliver);
      early = [];
    },
  };
}
