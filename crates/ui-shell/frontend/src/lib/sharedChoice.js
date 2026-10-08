// An app-wide choice the trader makes once for every window, such as the
// text size: chosen in one window, it is applied there, saved, and sent to
// the others. A choice made in this session, here or in another window, is
// newer than the save, so it wins when the save is read after it.

import { emit, listen } from "@tauri-apps/api/event";
import { debouncedSetter, getSetting } from "./settings.js";

/**
 * @template T
 * @param {object} spec
 * @param {string} spec.setting the settings key it is saved under
 * @param {string} spec.event the event that carries it between windows
 * @param {(value: unknown) => boolean} spec.isValid whether a value is one of the choices
 * @param {() => T} spec.current the choice in effect in this window
 * @param {(value: T) => void} spec.apply puts a choice into effect in this window
 */
export function sharedChoice({ setting, event, isValid, current, apply }) {
  /** Ignores this window's own events, which Tauri delivers back late. */
  const source = crypto.randomUUID();
  const save = debouncedSetter(setting, 0);
  /** Shows the trader a choice that did not reach the other windows; set by `start`. */
  let report = () => {};
  let chosen = false;
  let started = null;

  return {
    /** Applies `value` here, saves it, and sends it to every other window. */
    choose(value) {
      if (!isValid(value) || value === current()) return;
      chosen = true;
      apply(value);
      save(value);
      emit(event, { source, value }).catch((err) => report(String(err)));
    },

    /** Applies the choice in effect, then the save, and follows other windows; once per window. */
    start({ onError } = {}) {
      started ??= (async () => {
        report = onError ?? report;
        apply(current());
        await listen(event, ({ payload }) => {
          if (payload?.source === source || !isValid(payload?.value)) return;
          chosen = true;
          apply(payload.value);
        });
        const saved = await getSetting(setting);
        if (!chosen && isValid(saved)) apply(saved);
      })();
      return started;
    },
  };
}
