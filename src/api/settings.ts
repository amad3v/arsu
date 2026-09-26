// The settings rules the backend enforces (crates/storage/src/settings.rs),
// mirrored so the UI offers only values the backend accepts.

import type { AutoLockMinutes, Settings } from '@app-types/api';

/** Every auto-lock delay the backend accepts, in minutes, ascending. */
export const AUTO_LOCK_MINUTES: readonly AutoLockMinutes[] = [1, 2, 5, 10, 15, 30];

/** Bounds, inclusive, for how long a copied code stays on the clipboard. */
export const CLIPBOARD_CLEAR_SECONDS = { min: 10, max: 60 } as const;

/** In effect until the user changes a setting, and while the settings file can't be read. */
export const DEFAULT_SETTINGS: Readonly<Settings> = {
  theme: 'system',
  autoLockMinutes: 5,
  clipboardClearSeconds: 20,
};
