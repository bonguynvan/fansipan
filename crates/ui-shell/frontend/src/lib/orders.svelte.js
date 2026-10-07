// Order actions shared by grid cells, which bo-grid mounts itself and so
// cannot be handed callbacks as props.

import { invoke } from "@tauri-apps/api/core";
import { SvelteSet } from "svelte/reactivity";

export const cancelling = new SvelteSet();
export const blotterError = $state({ text: null });

export async function cancelOrder(id) {
  blotterError.text = null;
  cancelling.add(id);
  try {
    await invoke("cancel_order", { id });
  } catch (err) {
    blotterError.text = `#${id}: ${String(err)}`;
  } finally {
    cancelling.delete(id);
  }
}
