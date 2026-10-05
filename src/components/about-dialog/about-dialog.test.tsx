import { fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library';
import { beforeEach, describe, expect, it } from 'vitest';

import { SettingsMenu } from '@cpt/settings';
import { stubPrefersDark } from '@cpt/testing/color-scheme';
import { mockCommands } from '@cpt/testing/ipc';

import appIcon from '../../assets/app-icon.svg';

import type { AboutInfo, Settings } from '@app-types/api';
import type { CommandCall } from '@cpt/testing/ipc';

const settings: Settings = { theme: 'system', autoLockMinutes: 10, clipboardClearSeconds: 10 };

const about: AboutInfo = {
  name: 'Arsu',
  version: '1.0.0',
  tauriVersion: '2.11.6',
  webviewName: 'WebKitGTK',
  webviewVersion: null,
  vaultPath: '/home/u/.local/share/arsu/vault',
  settingsPath: '/home/u/.config/arsu/settings.json',
};

let calls: CommandCall[];

beforeEach(() => {
  stubPrefersDark(false);
  calls = mockCommands({
    get_settings: () => settings,
    get_about_info: () => about,
    open_link: () => null,
    copy_about_details: () => null,
  });
});

/**
 * Opens the About dialog from the settings menu, by keyboard: End moves to
 * the last item, Enter picks it. (jsdom can't highlight an item by hovering,
 * which a click needs.)
 */
async function chooseAbout(): Promise<HTMLElement> {
  fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
  const item = await screen.findByRole('menuitem', { name: 'About Arsu' });
  const menu = screen.getByRole('menu');
  fireEvent.keyDown(menu, { key: 'End' });
  await waitFor(() => {
    expect(item.hasAttribute('data-highlighted')).toBe(true);
  });
  fireEvent.keyDown(menu, { key: 'Enter' });
  return screen.findByRole('dialog', { name: 'About Arsu' });
}

async function openAbout(): Promise<HTMLElement> {
  render(() => <SettingsMenu />);
  return chooseAbout();
}

const commands = (cmd: string) => calls.filter((call) => call.cmd === cmd).map((call) => call.args);

describe('AboutDialog', () => {
  it('shows the version, the licence from LICENSE, and the details', async () => {
    const dialog = await openAbout();

    expect(await within(dialog).findByText('Version 1.0.0')).toBeTruthy();
    // The app's logo (decorative: its name is next to it), not an icon from the set.
    expect(dialog.querySelector('img[alt=""]')?.getAttribute('src')).toBe(appIcon);
    expect(
      within(dialog).getByText('MIT License · Copyright (c) 2026 Mohamed Jouini'),
    ).toBeTruthy();
    expect(within(dialog).getByText(/Permission is hereby granted/)).toBeTruthy();
    expect(within(dialog).getByText('/home/u/.local/share/arsu/vault')).toBeTruthy();
    expect(within(dialog).getByText('unknown')).toBeTruthy(); // no WebKitGTK version
  });

  it('shows no paths when the backend gives none, as on Android', async () => {
    mockCommands({
      get_settings: () => settings,
      get_about_info: () => ({ ...about, vaultPath: null, settingsPath: null }),
    });
    const dialog = await openAbout();

    await within(dialog).findByText('Version');
    expect(within(dialog).queryByText('Vault')).toBeNull();
    expect(within(dialog).queryByText('Settings')).toBeNull();
  });

  it('opens the project pages by name, never by address', async () => {
    const dialog = await openAbout();

    fireEvent.click(within(dialog).getByRole('button', { name: 'Visit website' }));
    fireEvent.click(within(dialog).getByRole('button', { name: 'Report an issue' }));

    await waitFor(() => {
      expect(commands('open_link')).toEqual([{ link: 'website' }, { link: 'issues' }]);
    });
  });

  it('copies the details, and gives focus back to the menu button as it closes', async () => {
    const dialog = await openAbout();

    fireEvent.click(within(dialog).getByRole('button', { name: 'Copy details' }));
    await waitFor(() => {
      expect(commands('copy_about_details')).toHaveLength(1);
    });

    fireEvent.click(within(dialog).getByRole('button', { name: 'Close' }));
    await waitFor(() => {
      expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Settings' }));
    });

    await waitFor(() => {
      expect(screen.queryByRole('dialog')).toBeNull();
    });

    // And it opens again, reading the details afresh.
    expect(await chooseAbout()).toBeTruthy();
    await waitFor(() => {
      expect(commands('get_about_info')).toHaveLength(2);
    });
  });
});
