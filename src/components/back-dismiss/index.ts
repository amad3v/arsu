// Android's Back button and back gesture for the overlays of the touch
// interface (pages, sheets, dialogs): Back closes the one on top, as in any
// Android app, rather than leaving the app with a dialog still open.
//
// The activity (MainActivity.kt) asks the page first, on every Back: it calls
// `window.__arsuBack()`, which closes the overlay on top and answers true, or
// answers false when none is open, and the activity then lets Android leave
// the app. This leaves the WebView's history alone: Android's WebView skips
// history entries a page adds by script, so Back through history could leave
// the app from an open page.

import { createEffect, onCleanup } from 'solid-js';

import type { Accessor } from 'solid-js';

declare global {
  interface Window {
    /** Called by the Android activity on Back: true if it closed an overlay. */
    __arsuBack?: () => boolean;
  }
}

interface Overlay {
  close: () => void;
}

/** The open overlays, the one on top last. */
const open: Overlay[] = [];

/** Closes the overlay on top; false when none is open. */
function back(): boolean {
  const top = open.pop();
  if (top === undefined) return false;
  top.close();
  return true;
}

/**
 * Registers an open overlay; Back closes it while it is on top. Returns the
 * function to call once it has closed, however it closed.
 */
export function registerOverlay(close: () => void): () => void {
  window.__arsuBack = back;
  const overlay: Overlay = { close };
  open.push(overlay);

  return () => {
    const index = open.indexOf(overlay);
    // Already gone: Back closed it.
    if (index !== -1) open.splice(index, 1);
  };
}

/**
 * Lets Back close an overlay while `isOpen` is true. `enabled` is read once:
 * the desktop interface has no Back button.
 */
export function createBackDismiss(
  isOpen: Accessor<boolean>,
  close: () => void,
  enabled: boolean,
): void {
  if (!enabled) return;
  createEffect(() => {
    if (!isOpen()) return;
    const release = registerOverlay(() => close());
    onCleanup(release);
  });
}
