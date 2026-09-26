import { fireEvent, render } from '@solidjs/testing-library';
import { emit } from '@tauri-apps/api/event';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it, vi } from 'vitest';

import { appError, mockCommands } from '@cpt/testing/ipc';

import {
  detectVaultScreen,
  onBackendLock,
  startFailure,
  suppressPageContextMenu,
  vaultScreen,
} from '.';

import type { Component } from 'solid-js';

describe('vaultScreen', () => {
  it('creates a vault when there is none, and unlocks one that is locked', () => {
    expect(vaultScreen(false, false)).toBe('create');
    expect(vaultScreen(true, false)).toBe('unlock');
    expect(vaultScreen(true, true)).toBe('unlocked');
  });
});

describe('detectVaultScreen', () => {
  it('asks whether the vault is unlocked only when it exists', async () => {
    const calls = mockCommands({ vault_exists: () => false });
    await expect(detectVaultScreen()).resolves.toBe('create');
    expect(calls.map((call) => call.cmd)).toEqual(['vault_exists']);
  });

  it('opens on the dashboard when the vault is already unlocked', async () => {
    mockCommands({ vault_exists: () => true, is_unlocked: () => true });
    await expect(detectVaultScreen()).resolves.toBe('unlocked');
  });
});

describe('startFailure', () => {
  it('says another instance has the vault open', () => {
    const failure = startFailure(appError('VaultInUse', 'the vault is in use by another process'));
    expect(failure.title).toBe('Arsu is already running');
    expect(failure.message).toBe('the vault is in use by another process');
  });

  it('says the vault could not be opened otherwise', () => {
    expect(startFailure(appError('StorageIo', 'permission denied')).title).toBe(
      "Arsu couldn't open the vault",
    );
  });
});

describe('onBackendLock', () => {
  it('calls back on each vault-locked event while its component lives', async () => {
    mockIPC(() => undefined, { shouldMockEvents: true });
    const onLocked = vi.fn();
    const Listener: Component = () => {
      onBackendLock(onLocked);
      return null;
    };

    const { unmount } = render(() => <Listener />);
    await vi.waitFor(async () => {
      await emit('vault-locked');
      expect(onLocked).toHaveBeenCalled();
    });

    unmount();
    onLocked.mockClear();
    await emit('vault-locked');
    expect(onLocked).not.toHaveBeenCalled();
  });
});

describe('suppressPageContextMenu', () => {
  it("turns off the page's menu, but not a text field's, until turned back on", () => {
    const stop = suppressPageContextMenu();
    const field = document.createElement('input');
    document.body.append(field);

    expect(fireEvent.contextMenu(document.body)).toBe(false);
    expect(fireEvent.contextMenu(field)).toBe(true);

    stop();
    expect(fireEvent.contextMenu(document.body)).toBe(true);
    field.remove();
  });
});
