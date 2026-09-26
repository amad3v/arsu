// The choices the settings menu offers, and the parsing of what a menu item
// hands back (always a string) into a value the backend accepts.

import { AUTO_LOCK_MINUTES, CLIPBOARD_CLEAR_SECONDS } from '@api/settings';
import { countLabel } from '@cpt/format';

import type { AutoLockMinutes, Settings, SettingsUpdate } from '@app-types/api';

/** The clipboard delays offered, in seconds; the backend accepts any whole number in range. */
export const CLIPBOARD_CLEAR_PRESETS: readonly number[] = [10, 15, 20, 30, 45, 60];

/**
 * The clipboard delays to offer: the presets, plus the current value if it is
 * none of them (a settings file edited by hand), so the menu can show it.
 */
export function clipboardClearChoices(current: number): number[] {
  if (CLIPBOARD_CLEAR_PRESETS.includes(current)) return [...CLIPBOARD_CLEAR_PRESETS];
  return [...CLIPBOARD_CLEAR_PRESETS, current].sort((a, b) => a - b);
}

export function minutesLabel(minutes: number): string {
  return countLabel(minutes, 'minute', 'minutes');
}

export function secondsLabel(seconds: number): string {
  return countLabel(seconds, 'second', 'seconds');
}

export function parseAutoLockMinutes(value: string): AutoLockMinutes | null {
  return AUTO_LOCK_MINUTES.find((minutes) => String(minutes) === value) ?? null;
}

export function parseClipboardClearSeconds(value: string): number | null {
  const seconds = Number(value);
  const inRange =
    Number.isInteger(seconds) &&
    seconds >= CLIPBOARD_CLEAR_SECONDS.min &&
    seconds <= CLIPBOARD_CLEAR_SECONDS.max;
  return inRange ? seconds : null;
}

/** `settings` with `change` applied, as the backend applies it: absent or null leaves a field as is. */
export function applySettingsUpdate(
  settings: Readonly<Settings>,
  change: SettingsUpdate,
): Settings {
  return {
    theme: change.theme ?? settings.theme,
    autoLockMinutes: change.autoLockMinutes ?? settings.autoLockMinutes,
    clipboardClearSeconds: change.clipboardClearSeconds ?? settings.clipboardClearSeconds,
  };
}
