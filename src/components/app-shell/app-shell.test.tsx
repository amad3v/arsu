import { fireEvent, render } from '@solidjs/testing-library';
import { emit } from '@tauri-apps/api/event';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it, vi } from 'vitest';

import { appError, mockCommands } from '@cpt/testing/ipc';

import {
  detectVaultScreen,
  onBackendLock,
  reportUiCrash,
  reportUncaughtErrors,
  startFailure,
  suppressPageContextMenu,
  suppressPinchZoom,
  uiError,
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

describe('suppressPinchZoom', () => {
  const touches = (count: number) => ({
    touches: Array.from({ length: count }, (_, identifier) => ({
      identifier,
      target: document.body,
    })),
  });

  it('takes a two-finger touch from the engine, never a one-finger one, until turned back on', () => {
    const stop = suppressPinchZoom();

    // One finger scrolls and taps as ever.
    expect(fireEvent.touchStart(document.body, touches(1))).toBe(true);
    expect(fireEvent.touchMove(document.body, touches(1))).toBe(true);
    // Two are a pinch, which the page keeps for itself (and does nothing with).
    expect(fireEvent.touchStart(document.body, touches(2))).toBe(false);
    expect(fireEvent.touchMove(document.body, touches(2))).toBe(false);

    stop();
    expect(fireEvent.touchMove(document.body, touches(2))).toBe(true);
  });
});

describe('uiError', () => {
  it('names an Error and keeps where it happened', () => {
    const error = new TypeError('x is undefined');

    expect(uiError(error, false)).toEqual({
      name: 'TypeError',
      message: 'x is undefined',
      stack: error.stack,
      fatal: false,
    });
  });

  it('says what a failed command reported, named by its kind', () => {
    expect(uiError(appError('FileWrite', 'could not write ~/a'), true)).toEqual({
      name: 'FileWrite',
      message: 'could not write ~/a',
      stack: null,
      fatal: true,
    });
  });

  it('takes anything else as text', () => {
    expect(uiError('boom', false)).toEqual({
      name: null,
      message: 'boom',
      stack: null,
      fatal: false,
    });
  });
});

describe('reporting to the diagnostic log', () => {
  it('reports uncaught errors and unhandled rejections until stopped', async () => {
    const calls = mockCommands({ report_ui_error: () => null });
    const stop = reportUncaughtErrors();

    window.dispatchEvent(new ErrorEvent('error', { error: new Error('broke'), message: 'broke' }));
    const rejection = new Event('unhandledrejection') as PromiseRejectionEvent;
    Object.defineProperty(rejection, 'reason', { value: 'nobody caught me' });
    window.dispatchEvent(rejection);
    stop();
    window.dispatchEvent(new ErrorEvent('error', { message: 'after stopping' }));

    await vi.waitFor(() => expect(calls).toHaveLength(2));
    expect(
      calls.map((call) => (call.args as { error: { message: string } }).error.message),
    ).toEqual(['broke', 'nobody caught me']);
  });

  it('reports a crash as fatal, and stays quiet if the report fails too', async () => {
    const calls = mockCommands({
      report_ui_error: () => {
        throw new Error('the backend is gone');
      },
    });

    reportUiCrash(new Error('render failed'));

    await vi.waitFor(() => expect(calls).toHaveLength(1));
    expect((calls[0].args as { error: { fatal: boolean } }).error.fatal).toBe(true);
  });
});
