import { createEffect, createMemo, on, untrack } from 'solid-js';
import { createStore } from 'solid-js/store';

import { getSettings, updateSettings } from '@api';
import { errorMessage } from '@api/lib';
import { DEFAULT_SETTINGS } from '@api/settings';
import { toaster } from '@cpt/toaster';

import { applySettingsUpdate } from './options';
import {
  applyColorScheme,
  cacheTheme,
  createPrefersDark,
  readCachedTheme,
  resolveTheme,
  transitionToColorScheme,
} from './theme';

import type { Settings, SettingsUpdate } from '@app-types/api';

export interface SettingsStore {
  /** The settings in effect. Reactive. */
  readonly values: Readonly<Settings>;
  /** Changes settings: the change shows at once, and is undone if it can't be saved. */
  update: (change: SettingsUpdate) => Promise<void>;
}

/**
 * The app's settings, loaded once. Create it under a reactive owner before the
 * first paint: it paints the theme cached by the last launch at once (no flash
 * of the wrong theme), then loads the saved settings and follows them, and the
 * OS preference while the theme is 'system'.
 */
export function createSettingsStore(): SettingsStore {
  const [values, setValues] = createStore<Settings>({
    ...DEFAULT_SETTINGS,
    theme: readCachedTheme() ?? DEFAULT_SETTINGS.theme,
  });
  // What the backend holds: what a change that can't be saved reverts to.
  let saved: Settings = { ...DEFAULT_SETTINGS };

  const prefersDark = createPrefersDark();
  const scheme = createMemo(() => resolveTheme(values.theme, prefersDark()));

  // Painted now, synchronously; from then on, each change crossfades.
  applyColorScheme(untrack(scheme));
  createEffect(on(scheme, transitionToColorScheme, { defer: true }));
  createEffect(() => {
    cacheTheme(values.theme);
  });

  void load();

  async function load() {
    try {
      saved = await getSettings();
    } catch (err) {
      // The backend runs on its defaults until a setting is saved.
      saved = { ...DEFAULT_SETTINGS };
      toaster.create({
        type: 'error',
        title: "Your settings couldn't be read",
        description: `${errorMessage(err)} The defaults are in effect; changing a setting saves a new settings file.`,
      });
    }
    setValues(saved);
  }

  async function update(change: SettingsUpdate) {
    setValues(applySettingsUpdate(values, change));

    try {
      saved = await updateSettings(change);
    } catch (err) {
      toaster.create({
        type: 'error',
        title: "The setting couldn't be saved",
        description: errorMessage(err),
      });
    }
    setValues(saved);
  }

  return { values, update };
}
