// One listener on the shell's conflated event, fanned out to every screen
// that wants it. The shell sends at most one batch per flush interval:
// { quotes: Quote[], orders: Order[], account: Account | null, bars: Bar[] }.

import { listen } from "@tauri-apps/api/event";

const EVENT = "bus-batch";
const subscribers = new Set();
let listening = null;

/** Starts listening once; later calls return the same promise. */
export function startBridge() {
  listening ??= listen(EVENT, (event) => {
    for (const subscriber of subscribers) subscriber(event.payload);
  });
  return listening;
}

/** Calls `subscriber(batch)` for every batch until the returned function is called. */
export function onBatch(subscriber) {
  subscribers.add(subscriber);
  return () => subscribers.delete(subscriber);
}
