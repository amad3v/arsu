import { fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { SettingsMenu } from '@cpt/settings';
import { stubPrefersDark } from '@cpt/testing/color-scheme';
import { dialogSettled } from '@cpt/testing/dialog';
import { appError, mockCommands } from '@cpt/testing/ipc';
import { GlobalToaster, toaster } from '@cpt/toaster';

import appIcon from '../../assets/app-icon.svg';

import type { AboutInfo, DiagnosticReportPreview, Settings } from '@app-types/api';
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

const report: DiagnosticReportPreview = {
  fileName: 'arsu-diagnostics-20261011-140322.zip',
  bytes: 12_345,
  environment: '{\n  "format": "arsu-diagnostic-report/1"\n}',
  records: 3,
  latest: '{"type":"env"}\n{"level":"warning","msg":"session failed"}\n{"seq":3}',
  latestRecords: 3,
};

let calls: CommandCall[];

beforeEach(() => {
  stubPrefersDark(false);
  calls = mockCommands({
    get_settings: () => settings,
    get_about_info: () => about,
    open_link: () => null,
    copy_about_details: () => null,
    prepare_diagnostic_report: () => report,
    save_diagnostic_report: () => true,
  });
});

// The toaster's toasts outlive a test's render: each test starts with none.
afterEach(() => {
  toaster.remove();
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

  /** Opens the About dialog, and from it the diagnostic report. */
  async function openReport(): Promise<HTMLElement> {
    const about = await openAbout();
    await dialogSettled();
    fireEvent.click(within(about).getByRole('button', { name: 'Save diagnostic report' }));
    return screen.findByRole('dialog', { name: 'Diagnostic report' });
  }

  it('shows the diagnostic report whole before saving it', async () => {
    render(() => <GlobalToaster />);
    const preview = await openReport();

    expect(
      within(preview).getByText('arsu-diagnostics-20261011-140322.zip · 12.3 kB · 3 records'),
    ).toBeTruthy();
    expect(within(preview).getByText(/"format": "arsu-diagnostic-report\/1"/)).toBeTruthy();
    expect(within(preview).getByText(/"msg":"session failed"/)).toBeTruthy();
    expect(within(preview).getByRole('heading', { name: 'The log' })).toBeTruthy();
    expect(commands('save_diagnostic_report')).toHaveLength(0);

    fireEvent.click(within(preview).getByRole('button', { name: 'Save report' }));

    expect(await screen.findByText('Diagnostic report saved')).toBeTruthy();
    expect(commands('save_diagnostic_report')).toHaveLength(1);
    await waitFor(() => {
      expect(screen.queryByRole('dialog', { name: 'Diagnostic report' })).toBeNull();
    });
  });

  it('says how much of a long log it shows', async () => {
    mockCommands({
      get_settings: () => settings,
      get_about_info: () => about,
      prepare_diagnostic_report: () => ({ ...report, records: 1200, latestRecords: 500 }),
    });
    const preview = await openReport();

    expect(
      within(preview).getByRole('heading', { name: 'The log: its latest 500 records' }),
    ).toBeTruthy();
  });

  it('says when the log is empty', async () => {
    mockCommands({
      get_settings: () => settings,
      get_about_info: () => about,
      prepare_diagnostic_report: () => ({ ...report, records: 0, latest: '', latestRecords: 0 }),
    });
    const preview = await openReport();

    expect(within(preview).getByText(/Nothing has gone wrong on this device yet/)).toBeTruthy();
  });

  it('keeps the report open when the save is cancelled, and saves nothing when it is', async () => {
    calls = mockCommands({
      get_settings: () => settings,
      get_about_info: () => about,
      prepare_diagnostic_report: () => report,
      save_diagnostic_report: () => false,
    });
    render(() => <GlobalToaster />);
    const preview = await openReport();

    fireEvent.click(within(preview).getByRole('button', { name: 'Save report' }));
    await waitFor(() => expect(commands('save_diagnostic_report')).toHaveLength(1));
    expect(screen.queryByText('Diagnostic report saved')).toBeNull();
    expect(screen.getByRole('dialog', { name: 'Diagnostic report' })).toBeTruthy();

    const cancel = within(preview).getByRole<HTMLButtonElement>('button', { name: 'Cancel' });
    // The buttons wait while the save dialog is open.
    await waitFor(() => expect(cancel.disabled).toBe(false));
    fireEvent.click(cancel);

    await waitFor(() => {
      expect(screen.queryByRole('dialog', { name: 'Diagnostic report' })).toBeNull();
    });
    expect(commands('save_diagnostic_report')).toHaveLength(1);
    expect(screen.getByRole('dialog', { name: 'About Arsu' })).toBeTruthy();
  });

  it('says why the report could not be prepared', async () => {
    mockCommands({
      get_settings: () => settings,
      get_about_info: () => about,
      prepare_diagnostic_report: () => {
        throw appError('NoDiagnosticLog', 'there is no diagnostic log on this device');
      },
    });
    render(() => <GlobalToaster />);
    const dialog = await openAbout();

    fireEvent.click(within(dialog).getByRole('button', { name: 'Save diagnostic report' }));

    expect(await screen.findByText('there is no diagnostic log on this device')).toBeTruthy();
    expect(screen.queryByRole('dialog', { name: 'Diagnostic report' })).toBeNull();
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
