// The workspace's extension points. Everything a user can put on screen is
// a registered screen, and every starting arrangement is a registered
// layout. The built-ins register through the same calls a third party
// uses (see builtins.js and docs/customizing.md).

const ID = /^[a-z][a-z0-9-]{0,39}$/;

const screens = new Map();
const layouts = new Map();

function checkId(kind, id, registry) {
  if (typeof id !== "string" || !ID.test(id)) {
    throw new Error(`${kind} id "${id}" must be 1-40 of a-z, 0-9, '-', starting with a letter`);
  }
  if (registry.has(id)) throw new Error(`${kind} "${id}" is already registered`);
}

/**
 * Registers a screen: a Svelte component a panel can show.
 *
 * The component receives two props:
 * - `params`: the panel's saved parameters (an object, saved with the layout);
 * - `panel`: the dockview panel API, for `panel.updateParameters({...})` and
 *   `panel.setTitle(...)`.
 *
 * @param {object} screen
 * @param {string} screen.id         Stable id, saved in layouts. Never rename a shipped one.
 * @param {string} screen.title      Default tab title: a message id (see src/i18n)
 *                                   or plain text.
 * @param {import("svelte").Component} screen.component
 * @param {object} [screen.defaultParams]  Parameters a new panel starts with.
 * @param {boolean} [screen.linkable]      Shows a link toggle; the screen follows
 *                                         the shared symbol while `params.linked`
 *                                         (see links.svelte.js).
 * @param {string} [screen.description]    One line for the "Add panel" menu.
 */
export function registerScreen(screen) {
  checkId("screen", screen?.id, screens);
  if (typeof screen.title !== "string" || !screen.title) throw new Error(`screen "${screen.id}" needs a title`);
  if (!screen.component) throw new Error(`screen "${screen.id}" needs a component`);
  screens.set(
    screen.id,
    Object.freeze({
      linkable: false,
      description: "",
      ...screen,
      defaultParams: Object.freeze({ ...(screen.defaultParams ?? {}) }),
    }),
  );
}

export function getScreen(id) {
  return screens.get(id);
}

export function listScreens() {
  return [...screens.values()];
}

/**
 * Registers a starting layout. `build(add)` arranges panels by calling
 * `add(screenId, options)`, which returns the new panel; options are
 * `{ params, title, position }` with dockview's `position`
 * (`{ referencePanel, direction: "left" | "right" | "above" | "below" | "within" }`).
 *
 * @param {object} layout
 * @param {string} layout.id
 * @param {string} layout.title
 * @param {(add: (screenId: string, options?: object) => object) => void} layout.build
 */
export function registerLayout(layout) {
  checkId("layout", layout?.id, layouts);
  if (typeof layout.build !== "function") throw new Error(`layout "${layout.id}" needs a build function`);
  layouts.set(layout.id, Object.freeze({ ...layout }));
}

export function getLayout(id) {
  return layouts.get(id);
}

export function listLayouts() {
  return [...layouts.values()];
}

/** Test-only: forget every registration. */
export function resetRegistryForTests() {
  screens.clear();
  layouts.clear();
}
