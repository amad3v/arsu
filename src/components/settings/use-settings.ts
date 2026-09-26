import { createRoot } from 'solid-js';

import { createSettingsStore } from './settings-store';

import type { SettingsStore } from './settings-store';

let settings: SettingsStore | undefined;

/**
 * The app's one settings store, shared by every screen. The first call creates
 * it, which paints the cached theme and loads the saved settings: App makes that
 * call before the first paint. It lives as long as the app, in its own root.
 */
export function useSettings(): SettingsStore {
  settings ??= createRoot(() => createSettingsStore());
  return settings;
}
