import { createEffect, createSignal, onCleanup } from 'solid-js';

import type { Accessor } from 'solid-js';

/** The delay from `now` to the next whole second, in ms. */
export function msUntilNextSecond(now: number): number {
  return 1000 - (now % 1000);
}

/**
 * The time in ms since the epoch, updated on each whole second while
 * `running()`: the moment a countdown's seconds change and a code expires.
 * The timer stops when `running()` turns false or the owner is disposed.
 */
export function createClock(running: Accessor<boolean>): Accessor<number> {
  const [now, setNow] = createSignal(Date.now());

  createEffect(() => {
    if (!running()) return;

    let timer: number | undefined;
    const tick = () => {
      const time = Date.now();
      setNow(time);
      timer = window.setTimeout(tick, msUntilNextSecond(time));
    };

    tick();
    onCleanup(() => {
      window.clearTimeout(timer);
    });
  });

  return now;
}
