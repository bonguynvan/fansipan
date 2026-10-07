// Mounts a registered screen's Svelte component as a dockview panel body,
// and keeps its `params` prop in step with the panel's parameters.

import { mount, unmount } from "svelte";
import MissingScreen from "./MissingScreen.svelte";
import { persist } from "./workspace.svelte.js";

export function createRenderer(screen, name) {
  const element = document.createElement("div");
  element.className = "panel-body";
  const props = $state({ params: {}, panel: null, name });
  let instance = null;

  return {
    element,
    init(parameters) {
      props.params = parameters.params ?? {};
      props.panel = parameters.api;
      instance = mount(screen?.component ?? MissingScreen, { target: element, props });
    },
    // A screen changing its own parameters (a symbol, a setting) changes
    // the saved layout too.
    update(event) {
      props.params = { ...props.params, ...(event.params ?? {}) };
      persist();
    },
    dispose() {
      if (instance) unmount(instance);
      instance = null;
    },
  };
}
