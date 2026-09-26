// A stand-in for window.matchMedia, which jsdom lacks, for the
// `prefers-color-scheme` query the settings store follows. Imported only by
// *.test.ts(x) files.

import { vi } from 'vitest';

export interface FakeColorSchemeQuery {
  /** Changes the OS preference, as switching the desktop's theme would. */
  setPrefersDark: (dark: boolean) => void;
}

/** Installs the stand-in; `vi.unstubAllGlobals()` removes it. */
export function stubPrefersDark(initiallyDark: boolean): FakeColorSchemeQuery {
  const listeners = new Set<() => void>();
  let matches = initiallyDark;

  vi.stubGlobal('matchMedia', (media: string) => ({
    media,
    get matches() {
      return matches;
    },
    addEventListener: (_type: 'change', listener: () => void) => {
      listeners.add(listener);
    },
    removeEventListener: (_type: 'change', listener: () => void) => {
      listeners.delete(listener);
    },
  }));

  return {
    setPrefersDark: (dark) => {
      matches = dark;
      for (const listener of listeners) listener();
    },
  };
}
