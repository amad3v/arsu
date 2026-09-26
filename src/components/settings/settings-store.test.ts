import { createRoot } from 'solid-js';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { DEFAULT_SETTINGS } from '@api/settings';
import { stubPrefersDark } from '@cpt/testing/color-scheme';
import { appError, deferred, mockCommands } from '@cpt/testing/ipc';
import { toaster } from '@cpt/toaster';

import { createSettingsStore } from './settings-store';
import { cacheTheme, readCachedTheme } from './theme';

import type { SettingsStore } from './settings-store';
import type { Settings } from '@app-types/api';

const saved: Settings = { theme: 'light', autoLockMinutes: 15, clipboardClearSeconds: 30 };

/** Lets pending IPC promises settle. */
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

function mount(): { store: SettingsStore; dispose: () => void } {
  return createRoot((dispose) => ({ store: createSettingsStore(), dispose }));
}

const isDark = () => document.documentElement.classList.contains('dark');

afterEach(() => {
  localStorage.clear();
  document.documentElement.classList.remove('dark');
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('createSettingsStore', () => {
  it('paints the cached theme at once, before the saved settings arrive', async () => {
    stubPrefersDark(false);
    cacheTheme('dark');
    const answer = deferred<Settings>();
    mockCommands({ get_settings: () => answer.promise });

    const { store, dispose } = mount();
    expect(isDark()).toBe(true);
    expect(store.values.theme).toBe('dark');

    answer.resolve(saved);
    await settle();
    expect(store.values).toEqual(saved);
    expect(isDark()).toBe(false);
    expect(readCachedTheme()).toBe('light');
    dispose();
  });

  it('follows the OS while the theme is system', async () => {
    const os = stubPrefersDark(false);
    mockCommands({ get_settings: () => DEFAULT_SETTINGS });
    const { dispose } = mount();
    await settle();
    expect(isDark()).toBe(false);

    os.setPrefersDark(true);
    expect(isDark()).toBe(true);
    dispose();
  });

  it('falls back to the defaults, and says so, when the settings file is unreadable', async () => {
    stubPrefersDark(true);
    cacheTheme('light');
    const toast = vi.spyOn(toaster, 'create');
    mockCommands({
      get_settings: () => {
        throw appError('InvalidSettingsFile', 'settings.json: expected value at line 1');
      },
    });

    const { store, dispose } = mount();
    await settle();

    expect(store.values).toEqual(DEFAULT_SETTINGS);
    expect(isDark()).toBe(true);
    expect(toast).toHaveBeenCalledOnce();
    expect(toast.mock.calls[0][0].description).toContain('settings.json: expected value at line 1');
    dispose();
  });

  it('shows a change at once and keeps what the backend saved', async () => {
    stubPrefersDark(false);
    const answer = deferred<Settings>();
    const calls = mockCommands({
      get_settings: () => saved,
      update_settings: () => answer.promise,
    });
    const { store, dispose } = mount();
    await settle();

    const saving = store.update({ autoLockMinutes: 30 });
    expect(store.values.autoLockMinutes).toBe(30);

    answer.resolve({ ...saved, autoLockMinutes: 30 });
    await saving;
    expect(store.values).toEqual({ ...saved, autoLockMinutes: 30 });
    expect(calls[calls.length - 1]).toEqual({
      cmd: 'update_settings',
      args: { update: { autoLockMinutes: 30 } },
    });
    dispose();
  });

  it('undoes a change that cannot be saved, and says so', async () => {
    stubPrefersDark(false);
    const toast = vi.spyOn(toaster, 'create');
    mockCommands({
      get_settings: () => saved,
      update_settings: () => {
        throw appError('StorageIo', 'read-only file system');
      },
    });
    const { store, dispose } = mount();
    await settle();

    await store.update({ theme: 'dark' });

    expect(store.values).toEqual(saved);
    expect(isDark()).toBe(false);
    expect(toast).toHaveBeenCalledWith(
      expect.objectContaining({ type: 'error', description: 'read-only file system' }),
    );
    dispose();
  });
});
