import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  applyColorScheme,
  cacheTheme,
  isTheme,
  readCachedTheme,
  resolveTheme,
  transitionToColorScheme,
} from './theme';

afterEach(() => {
  localStorage.clear();
  document.documentElement.classList.remove('dark');
  vi.unstubAllGlobals();
});

describe('isTheme', () => {
  it('accepts exactly the three themes', () => {
    expect(isTheme('light')).toBe(true);
    expect(isTheme('dark')).toBe(true);
    expect(isTheme('system')).toBe(true);
    expect(isTheme('Dark')).toBe(false);
    expect(isTheme(null)).toBe(false);
  });
});

describe('resolveTheme', () => {
  it('follows the OS for system, and the choice otherwise', () => {
    expect(resolveTheme('system', true)).toBe('dark');
    expect(resolveTheme('system', false)).toBe('light');
    expect(resolveTheme('light', true)).toBe('light');
    expect(resolveTheme('dark', false)).toBe('dark');
  });
});

describe('the theme cache', () => {
  it('reads back what was cached', () => {
    cacheTheme('dark');
    expect(readCachedTheme()).toBe('dark');
  });

  it('is empty at first, and ignores a value that is not a theme', () => {
    expect(readCachedTheme()).toBeNull();
    localStorage.setItem('theme_cache', 'purple');
    expect(readCachedTheme()).toBeNull();
  });
});

describe('applyColorScheme', () => {
  it('switches the dark class on <html>', () => {
    applyColorScheme('dark');
    expect(document.documentElement.classList.contains('dark')).toBe(true);
    applyColorScheme('light');
    expect(document.documentElement.classList.contains('dark')).toBe(false);
  });
});

describe('transitionToColorScheme', () => {
  it('crossfades through a view transition where the WebView has them', () => {
    const startViewTransition = vi.fn((update: () => void) => {
      update();
    });
    Object.defineProperty(document, 'startViewTransition', {
      configurable: true,
      value: startViewTransition,
    });

    try {
      transitionToColorScheme('dark');
    } finally {
      Reflect.deleteProperty(document, 'startViewTransition');
    }

    expect(startViewTransition).toHaveBeenCalledOnce();
    expect(document.documentElement.classList.contains('dark')).toBe(true);
  });

  it('switches at once where it has none', () => {
    expect('startViewTransition' in document).toBe(false);
    transitionToColorScheme('dark');
    expect(document.documentElement.classList.contains('dark')).toBe(true);
  });
});
