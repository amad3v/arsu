import { createSignal, onCleanup } from 'solid-js';

import type { Accessor } from 'solid-js';

/**
 * A value that clears itself `durationMs` after it was last set, for
 * short-lived feedback ("Copied", "New"). Setting it again restarts the time.
 */
export function createFlash<T>(
  durationMs: number,
): [value: Accessor<T | null>, flash: (value: T) => void] {
  const [value, setValue] = createSignal<T | null>(null);
  let timer: number | undefined;

  const stop = () => {
    window.clearTimeout(timer);
  };
  onCleanup(stop);

  const flash = (next: T) => {
    stop();
    setValue(() => next);
    timer = window.setTimeout(() => setValue(null), durationMs);
  };

  return [value, flash];
}
