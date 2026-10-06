import { fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library';
import { Show } from 'solid-js';
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

import { stubPrefersDark } from '@cpt/testing/color-scheme';
import { mockCommands } from '@cpt/testing/ipc';

import { TouchDashboard } from '.';

import type { CodeResponse, EntrySummary } from '@app-types/api';
import type { DeleteEntryProps } from '@cpt/delete-entry';
import type * as ExportDialogs from '@cpt/export-dialogs';
import type { ExportEntryQrModalProps, ExportVaultModalProps } from '@cpt/export-dialogs';
import type { CommandCall } from '@cpt/testing/ipc';

// The dialogs belong to their own components and tests; here they render nothing.
vi.mock('@cpt/add-entry-dialog', () => ({ AddEntryModal: () => null }));
vi.mock('@cpt/import-dialog', () => ({ ImportModal: () => null }));
// The export dialog is only a button that reports a saved backup.
vi.mock('@cpt/export-dialogs', async (importOriginal) => ({
  ...(await importOriginal<typeof ExportDialogs>()),
  ExportVaultModal: (props: ExportVaultModalProps) => (
    <Show when={props.open}>
      <button type={'button'} onClick={() => props.onResult?.()}>
        {'Save the backup'}
      </button>
    </Show>
  ),
  // The QR code and delete dialogs only say which entry they were opened for.
  ExportEntryQrModal: (props: ExportEntryQrModalProps) => (
    <Show when={props.entry}>{(entry) => <p>{`QR code for ${entry().issuer}`}</p>}</Show>
  ),
}));
vi.mock('@cpt/delete-entry', () => ({
  DeleteEntry: (props: DeleteEntryProps) => (
    <Show when={props.entry}>{(entry) => <p>{`Delete ${entry().issuer}?`}</p>}</Show>
  ),
}));
vi.mock('@cpt/about-dialog', () => ({ AboutDialog: () => null }));

const entries: EntrySummary[] = [
  { id: 'gh', issuer: 'GitHub', accountLabel: 'alice', otpType: 'totp', digits: 6, period: 30 },
  { id: 'aws', issuer: 'AWS', accountLabel: 'root', otpType: 'hotp', digits: 6, period: null },
];

const codes: Record<string, CodeResponse> = {
  gh: { code: '123456', expiresInSeconds: 20, nextCode: '654321' },
  aws: { code: '999000', expiresInSeconds: null, nextCode: null },
};

let calls: CommandCall[];
const onLocked = vi.fn();
const called = (cmd: string) => calls.filter((call) => call.cmd === cmd).map((call) => call.args);

async function renderDashboard() {
  render(() => <TouchDashboard onLocked={onLocked} />);
  await screen.findByRole('button', { name: /Copy the code for GitHub/ });
}

beforeAll(() => {
  Element.prototype.scrollIntoView = vi.fn();
});

afterAll(() => {
  Reflect.deleteProperty(Element.prototype, 'scrollIntoView');
});

beforeEach(() => {
  onLocked.mockClear();
  stubPrefersDark(false);
  calls = mockCommands({
    get_settings: () => ({ theme: 'system', autoLockMinutes: 5, clipboardClearSeconds: 20 }),
    update_settings: () => ({ theme: 'system', autoLockMinutes: 15, clipboardClearSeconds: 20 }),
    list_entries: () => entries,
    get_current_code: (args) =>
      codes[args !== undefined && 'entryId' in args ? String(args.entryId) : ''],
    copy_code: () => null,
    lock_vault: () => null,
  });
});

describe('TouchDashboard', () => {
  it('copies a code with a tap on its row', async () => {
    await renderDashboard();

    fireEvent.click(screen.getByRole('button', { name: /Copy the code for GitHub/ }));

    await waitFor(() => expect(called('copy_code')).toEqual([{ code: '123456' }]));
    expect(await screen.findByText('Copied')).toBeTruthy();
  });

  it('generates an HOTP code only on a tap, then copies it', async () => {
    await renderDashboard();
    expect(called('get_current_code')).toEqual([{ entryId: 'gh' }]);

    fireEvent.click(
      screen.getByRole('button', { name: 'Generate and copy a code for AWS (root)' }),
    );

    await waitFor(() => expect(called('copy_code')).toEqual([{ code: '999000' }]));
    expect(called('get_current_code')).toContainEqual({ entryId: 'aws' });
  });

  it("opens an entry's actions in a sheet", async () => {
    await renderDashboard();

    fireEvent.click(screen.getByRole('button', { name: 'More actions for AWS (root)' }));

    const sheet = await screen.findByRole('dialog', { name: 'AWS (root)' });
    expect(within(sheet).getByRole('button', { name: 'Generate the next code' })).toBeTruthy();
    expect(within(sheet).getByRole('button', { name: 'Show QR code' })).toBeTruthy();
    expect(within(sheet).getByRole('button', { name: 'Delete' })).toBeTruthy();
  });

  it.each([
    ['Delete', 'Delete GitHub?'],
    ['Show QR code', 'QR code for GitHub'],
  ])('replaces the actions sheet with what %s opens', async (action, opened) => {
    await renderDashboard();

    fireEvent.click(screen.getByRole('button', { name: 'More actions for GitHub (alice)' }));
    const sheet = await screen.findByRole('dialog', { name: 'GitHub (alice)' });
    fireEvent.click(within(sheet).getByRole('button', { name: action }));

    expect(await screen.findByText(opened)).toBeTruthy();
    expect(screen.queryByRole('dialog', { name: 'GitHub (alice)' })).toBeNull();
  });

  it('searches by issuer or account', async () => {
    await renderDashboard();

    fireEvent.click(screen.getByRole('button', { name: 'Search entries' }));
    fireEvent.input(screen.getByRole('textbox', { name: 'Search entries' }), {
      target: { value: 'git' },
    });

    expect(screen.getByRole('button', { name: /Copy the code for GitHub/ })).toBeTruthy();
    expect(screen.queryByRole('button', { name: /for AWS/ })).toBeNull();
    expect(screen.getByRole('button', { name: 'Close search' })).toBeTruthy();
  });

  it('adds, imports and exports from the + button, not the settings', async () => {
    await renderDashboard();

    fireEvent.click(screen.getByRole('button', { name: 'Add, import or export' }));
    const sheet = await screen.findByRole('dialog', { name: 'Add or move entries' });
    for (const action of [/^Add entry/, /^Import entries/, /^Export encrypted backup/]) {
      expect(within(sheet).getByRole('button', { name: action })).toBeTruthy();
    }

    fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
    const page = await screen.findByRole('dialog', { name: 'Settings' });
    expect(within(page).queryByRole('button', { name: /Import entries/ })).toBeNull();
    expect(within(page).queryByRole('button', { name: /Export/ })).toBeNull();
  });

  it('says a saved backup in a sheet, as an import, once its dialog closes', async () => {
    await renderDashboard();

    fireEvent.click(screen.getByRole('button', { name: 'Add, import or export' }));
    const sheet = await screen.findByRole('dialog', { name: 'Add or move entries' });
    fireEvent.click(within(sheet).getByRole('button', { name: /^Export encrypted backup/ }));
    fireEvent.click(await screen.findByRole('button', { name: 'Save the backup' }));

    const result = await screen.findByRole('dialog', { name: 'Export finished' });
    expect(within(result).getByRole('heading', { name: 'Backup saved' })).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Save the backup' })).toBeNull();
  });

  it('locks the vault', async () => {
    await renderDashboard();

    fireEvent.click(screen.getByRole('button', { name: 'Lock vault' }));

    await waitFor(() => expect(onLocked).toHaveBeenCalledOnce());
    expect(called('lock_vault')).toHaveLength(1);
  });

  it('changes a setting from its page, through a sheet of choices', async () => {
    await renderDashboard();

    fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
    const page = await screen.findByRole('dialog', { name: 'Settings' });
    fireEvent.click(within(page).getByRole('button', { name: /Lock when idle for/ }));
    const choices = await screen.findByRole('radiogroup', { name: 'Lock when idle for' });
    expect(
      within(choices).getByRole('radio', { name: '5 minutes' }).getAttribute('aria-checked'),
    ).toBe('true');
    fireEvent.click(within(choices).getByRole('radio', { name: '15 minutes' }));

    await waitFor(() =>
      expect(called('update_settings')).toEqual([{ update: { autoLockMinutes: 15 } }]),
    );
  });
});
