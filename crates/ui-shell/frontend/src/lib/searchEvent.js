// The "app.search" action asks the title bar's search box for the keyboard
// with this window event, so the action needs no handle on the component.

export const SEARCH_EVENT = "fansipan:search";

export function openSearch() {
  window.dispatchEvent(new Event(SEARCH_EVENT));
}
