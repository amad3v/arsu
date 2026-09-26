import { fireEvent, render, screen } from '@solidjs/testing-library';
import { emit } from '@tauri-apps/api/event';
import { mockIPC } from '@tauri-apps/api/mocks';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { stubPrefersDark } from '@cpt/testing/color-scheme';
import { appError } from '@cpt/testing/ipc';

import App from './App';

// The screens have their own tests; here each is a line of text.
const screens = vi.hoisted(() => ({ dashboardFails: false }));
vi.mock('@cpt/auth-screens', () => ({
  CreateVaultScreen: () => 'create screen',
  UnlockScreen: () => 'unlock screen',
}));
vi.mock('@cpt/dashboard', () => ({
  Dashboard: () => {
    if (screens.dashboardFails) throw new Error('render failed');
    return 'dashboard';
  },
}));

/** Answers the start-up commands; `vault_exists` as given. */
function mockBackend(vaultExists: () => boolean, unlocked = false) {
  mockIPC(
    (cmd) => {
      switch (cmd) {
        case 'get_settings':
          return { theme: 'system', autoLockMinutes: 5, clipboardClearSeconds: 20 };
        case 'vault_exists':
          return vaultExists();
        case 'is_unlocked':
          return unlocked;
        default:
          throw new Error(`unexpected command: ${cmd}`);
      }
    },
    { shouldMockEvents: true },
  );
}

beforeEach(() => {
  screens.dashboardFails = false;
  stubPrefersDark(false);
});

describe('App', () => {
  it('opens on the screen the vault calls for', async () => {
    mockBackend(() => false);
    render(() => <App />);
    expect(await screen.findByText('create screen')).toBeTruthy();
  });

  it('shows why the vault could not be opened, and tries again', async () => {
    let failing = true;
    mockBackend(() => {
      if (failing) throw appError('StorageIo', 'permission denied');
      return true;
    });
    render(() => <App />);

    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toContain("Arsu couldn't open the vault");
    expect(alert.textContent).toContain('permission denied');

    failing = false;
    fireEvent.click(screen.getByRole('button', { name: 'Try again' }));
    expect(await screen.findByText('unlock screen')).toBeTruthy();
  });

  it('returns to the unlock screen when the backend locks the vault', async () => {
    mockBackend(() => true, true);
    render(() => <App />);
    expect(await screen.findByText('dashboard')).toBeTruthy();

    await emit('vault-locked');
    expect(await screen.findByText('unlock screen')).toBeTruthy();
  });

  it('catches a screen that fails to render, and offers to try again', async () => {
    screens.dashboardFails = true;
    mockBackend(() => true, true);
    render(() => <App />);

    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toContain('Something went wrong');
    expect(alert.textContent).toContain('render failed');

    screens.dashboardFails = false;
    fireEvent.click(screen.getByRole('button', { name: 'Try again' }));
    expect(await screen.findByText('dashboard')).toBeTruthy();
  });
});
