import { createSignal, onCleanup } from 'solid-js';

import type { Theme } from '@app-types/api';
import type { Accessor } from 'solid-js';

/** Every theme, in the order the theme menu lists them. */
export const THEMES: readonly Theme[] = ['light', 'dark', 'system'];

/** What a theme paints: 'system' resolves to the OS preference. */
export type ColorScheme = 'light' | 'dark';

export function isTheme(value: unknown): value is Theme {
  return THEMES.some((theme) => theme === value);
}

export function resolveTheme(theme: Theme, systemPrefersDark: boolean): ColorScheme {
  if (theme === 'system') return systemPrefersDark ? 'dark' : 'light';
  return theme;
}

/**
 * Where the last theme is kept, so that a launch paints it before the saved
 * settings arrive over IPC. A theme is not a secret.
 */
const THEME_CACHE_KEY = 'theme_cache';

/** The theme cached by the last launch, or null if there is none or it's not a theme. */
export function readCachedTheme(storage: Storage = localStorage): Theme | null {
  const cached = storage.getItem(THEME_CACHE_KEY);
  return isTheme(cached) ? cached : null;
}

export function cacheTheme(theme: Theme, storage: Storage = localStorage): void {
  storage.setItem(THEME_CACHE_KEY, theme);
}

/** Paints `scheme` at once: the `.dark` class switches every colour token (src/index.css). */
export function applyColorScheme(
  scheme: ColorScheme,
  root: HTMLElement = document.documentElement,
): void {
  root.classList.toggle('dark', scheme === 'dark');
}

/**
 * Paints `scheme` as a crossfade of the whole window, so that every element
 * changes together; at once where the WebView has no view transitions.
 */
export function transitionToColorScheme(scheme: ColorScheme): void {
  if ('startViewTransition' in document) {
    document.startViewTransition(() => {
      applyColorScheme(scheme);
    });
  } else {
    applyColorScheme(scheme);
  }
}

/** Whether the OS prefers dark, following changes until the owner is disposed. */
export function createPrefersDark(): Accessor<boolean> {
  const query = window.matchMedia('(prefers-color-scheme: dark)');
  const [prefersDark, setPrefersDark] = createSignal(query.matches);
  const onChange = () => setPrefersDark(query.matches);

  query.addEventListener('change', onChange);
  onCleanup(() => {
    query.removeEventListener('change', onChange);
  });

  return prefersDark;
}
