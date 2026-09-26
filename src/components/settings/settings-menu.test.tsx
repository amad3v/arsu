import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library';
import { beforeEach, describe, expect, it } from 'vitest';

import { stubPrefersDark } from '@cpt/testing/color-scheme';
import { mockCommands } from '@cpt/testing/ipc';

import { SettingsMenu } from './settings-menu';

import type { Settings } from '@app-types/api';
import type { CommandCall } from '@cpt/testing/ipc';

const saved: Settings = { theme: 'system', autoLockMinutes: 10, clipboardClearSeconds: 10 };

let calls: CommandCall[];

beforeEach(() => {
  stubPrefersDark(false);
  calls = mockCommands({
    get_settings: () => saved,
    update_settings: () => saved,
  });
});

async function openSettings() {
  render(() => <SettingsMenu />);
  fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
  return screen.findByRole('menu');
}

describe('SettingsMenu', () => {
  it('lists one entry per setting, each with its current value, then About', async () => {
    await openSettings();

    const entries = await screen.findAllByRole('menuitem');
    expect(entries.map((entry) => entry.textContent)).toEqual([
      'Lock when idle for10 minutes',
      'Clear copied codes after10 seconds',
      'About Arsu',
    ]);
    // The values are in the submenus, not listed flat in the one menu.
    expect(screen.queryAllByRole('menuitemradio')).toHaveLength(0);
  });

  it('opens a submenu of values, highlighting only the item under the pointer', async () => {
    await openSettings();

    fireEvent.click(await screen.findByRole('menuitem', { name: /Lock when idle for/ }));
    const tenMinutes = await screen.findByRole('menuitemradio', { name: '10 minutes' });
    expect(screen.queryByRole('menuitemradio', { name: '10 seconds' })).toBeNull();

    fireEvent.pointerMove(tenMinutes, { pointerType: 'mouse' });
    // The parent entry stays highlighted while its submenu is open, as in a native menu.
    await waitFor(() => {
      const highlighted = document.querySelectorAll('[role="menuitemradio"][data-highlighted]');
      expect([...highlighted].map((item) => item.textContent)).toEqual(['10 minutes']);
    });

    fireEvent.click(screen.getByRole('menuitemradio', { name: '15 minutes' }));
    await waitFor(() => {
      expect(calls.filter((call) => call.cmd === 'update_settings').map((c) => c.args)).toEqual([
        { update: { autoLockMinutes: 15 } },
      ]);
    });
  });
});
