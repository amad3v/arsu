// The frontend half of the auto-lock protocol:
// the backend owns the idle timer and locks on its own; the frontend only
// reports that the user is still there.

import { recordActivity } from './index';

/** The input that counts as the user being active. */
const ACTIVITY_EVENTS = ['keydown', 'pointerdown', 'wheel'] as const;

/** The backend hears about activity at most this often. */
export const ACTIVITY_REPORT_INTERVAL_MS = 15_000;

/**
 * Reports keyboard, pointer and wheel input on `target` to the backend's
 * auto-lock, at most once per `ACTIVITY_REPORT_INTERVAL_MS`. Call it once for
 * the app; it returns the function that stops reporting.
 */
export function reportUserActivity(target: EventTarget = window): () => void {
  // Monotonic, unlike Date.now(): setting the clock back can't stall reports.
  let lastReport = Number.NEGATIVE_INFINITY;

  const report = () => {
    const now = performance.now();
    if (now - lastReport < ACTIVITY_REPORT_INTERVAL_MS) return;
    lastReport = now;
    recordActivity().catch(() => {
      // The backend didn't record it: let the next input report again.
      lastReport = Number.NEGATIVE_INFINITY;
    });
  };

  // Capture phase, so input that a component stops from propagating (Escape
  // in a dialog, arrow keys in a menu) still counts.
  const options: AddEventListenerOptions = { capture: true, passive: true };
  for (const type of ACTIVITY_EVENTS) target.addEventListener(type, report, options);

  return () => {
    for (const type of ACTIVITY_EVENTS) target.removeEventListener(type, report, options);
  };
}
