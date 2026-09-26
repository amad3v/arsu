import { createMemo } from 'solid-js';

import type { Accessor } from 'solid-js';

/**
 * What a dialog is about, kept after its owner clears it.
 *
 * A dialog such as "Delete GitHub (alice)?" opens when its owner sets a
 * subject and closes when the owner sets it back to null. The content stays
 * on screen while the exit animation plays, so it must keep showing the entry
 * it was opened for rather than blanking out. This follows `subject` while it
 * is set and keeps the last value once it is null.
 */
export function createDialogSubject<T>(subject: Accessor<T | null>): Accessor<T | null> {
  return createMemo<T | null>((previous) => subject() ?? previous, null);
}
