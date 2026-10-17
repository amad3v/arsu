// Listening to the backend's events for as long as a component is mounted.

import { onCleanup, onMount } from 'solid-js';

import type { UnlistenFn } from '@tauri-apps/api/event';

/**
 * Subscribes once the component mounts, and unsubscribes when it unmounts —
 * synchronously, even when the subscription resolves only after the
 * component is gone. `onError` hears of a subscription that fails.
 */
export function listenWhileMounted(
  subscribe: () => Promise<UnlistenFn>,
  onError: (err: unknown) => void,
): void {
  onMount(() => {
    let unlisten: UnlistenFn | undefined;
    let disposed = false;

    subscribe().then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    }, onError);

    onCleanup(() => {
      disposed = true;
      unlisten?.();
    });
  });
}
