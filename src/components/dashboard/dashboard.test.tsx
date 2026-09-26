import { fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library';
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

import { stubPrefersDark } from '@cpt/testing/color-scheme';
import { mockCommands } from '@cpt/testing/ipc';

import { Dashboard } from '.';

import type { CodeResponse, EntrySummary } from '@app-types/api';
import type { CommandCall } from '@cpt/testing/ipc';

// The dialogs belong to their own components and tests; here they render nothing.
vi.mock('@cpt/add-entry-dialog', () => ({ AddEntryDialog: () => null }));
vi.mock('@cpt/import-dialog', () => ({ ImportDialog: () => null }));
vi.mock('@cpt/export-dialogs', () => ({
  ExportVaultDialog: () => null,
  ExportEntryQrModal: () => null,
}));
vi.mock('@cpt/delete-entry', () => ({ DeleteEntry: () => null }));

const entries: EntrySummary[] = [
  { id: 'gh', issuer: 'GitHub', accountLabel: 'alice', otpType: 'totp', digits: 6, period: 30 },
  {
    id: 'bob',
    issuer: null,
    accountLabel: 'bob@example.com',
    otpType: 'totp',
    digits: 8,
    period: 60,
  },
  { id: 'aws', issuer: 'AWS', accountLabel: 'root', otpType: 'hotp', digits: 6, period: null },
];

const codes: Record<string, CodeResponse> = {
  gh: { code: '123456', expiresInSeconds: 20, nextCode: '654321' },
  bob: { code: '11112222', expiresInSeconds: 50, nextCode: '33334444' },
  aws: { code: '999000', expiresInSeconds: null, nextCode: null },
};

let calls: CommandCall[];
const onLocked = vi.fn();

const searchField = () => screen.getByRole('combobox', { name: 'Search entries' });
const rowTitles = () =>
  screen.getAllByRole('row').map((row) => row.querySelector('.font-medium span')?.textContent);
const copied = () => calls.filter((call) => call.cmd === 'copy_code').map((call) => call.args);

async function renderDashboard() {
  render(() => <Dashboard onLocked={onLocked} />);
  // Wait until both TOTP codes are on screen.
  await screen.findByRole('button', { name: /Copy the code for GitHub/ });
  await screen.findByRole('button', { name: /Copy the code for bob@example.com/ });
}

beforeAll(() => {
  // jsdom has no layout, so no scrolling.
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
    list_entries: () => entries,
    get_current_code: (args) =>
      codes[args !== undefined && 'entryId' in args ? String(args.entryId) : ''],
    copy_code: () => null,
    lock_vault: () => null,
  });
});

describe('Dashboard', () => {
  it('lists the entries by issuer, then account, with the search focused', async () => {
    await renderDashboard();

    expect(rowTitles()).toEqual(['AWS', 'bob@example.com', 'GitHub']);
    expect(document.activeElement).toBe(searchField());
    expect(screen.getByText('3 entries')).toBeTruthy();
  });

  it('fetches TOTP codes, but never an HOTP code the user has not asked for', async () => {
    await renderDashboard();

    const fetched = calls
      .filter((call) => call.cmd === 'get_current_code')
      .map((call) => call.args);
    expect(fetched).toHaveLength(2);
    expect(fetched).toEqual(expect.arrayContaining([{ entryId: 'bob' }, { entryId: 'gh' }]));
  });

  it('filters as the user types, and copies the top match on Enter', async () => {
    await renderDashboard();

    fireEvent.input(searchField(), { target: { value: 'GIT' } });
    expect(rowTitles()).toEqual(['GitHub']);
    expect(screen.getByText('1 of 3 entries')).toBeTruthy();

    fireEvent.keyDown(searchField(), { key: 'Enter' });
    await waitFor(() => {
      expect(screen.getByRole('status').textContent).toBe(
        'Copied the code for GitHub (alice). The clipboard clears in 20 seconds.',
      );
    });
    expect(copied()).toEqual([{ code: '123456' }]);
  });

  it('moves the highlight with the arrow keys', async () => {
    await renderDashboard();

    fireEvent.keyDown(searchField(), { key: 'ArrowDown' });
    const highlighted = screen
      .getAllByRole('row')
      .filter((row) => row.getAttribute('aria-current'));
    expect(highlighted.map((row) => within(row).getByText('bob@example.com'))).toHaveLength(1);
    // Exposed to assistive tech, not just visually: the search
    // field's active descendant follows the arrow keys.
    expect(searchField().getAttribute('aria-activedescendant')).toBe(highlighted[0]?.id);

    fireEvent.keyDown(searchField(), { key: 'Enter' });
    await waitFor(() => expect(copied()).toEqual([{ code: '11112222' }]));
  });

  it('selects a row on click and copies its code on double-click', async () => {
    await renderDashboard();
    searchField().blur();

    const bob = screen.getAllByRole('row')[1];
    fireEvent.mouseDown(within(bob).getByText('bob@example.com'));
    expect(document.activeElement).toBe(searchField());
    expect(bob.getAttribute('aria-current')).toBe('true');

    fireEvent.dblClick(within(bob).getByText('bob@example.com'));
    await waitFor(() => expect(copied()).toEqual([{ code: '11112222' }]));
  });

  it('generates and copies an HOTP code on Enter', async () => {
    await renderDashboard();

    fireEvent.keyDown(searchField(), { key: 'Enter' });
    await waitFor(() => expect(copied()).toEqual([{ code: '999000' }]));
    expect(
      screen.getByRole('button', { name: 'Copy the code for AWS (root): 999 000' }),
    ).toBeTruthy();
  });

  it('clears the search on Escape', async () => {
    await renderDashboard();

    fireEvent.input(searchField(), { target: { value: 'nothing like this' } });
    expect(screen.getByText('No entries match “nothing like this”')).toBeTruthy();

    fireEvent.keyDown(searchField(), { key: 'Escape' });
    expect(rowTitles()).toHaveLength(3);
  });

  it('takes "/" in the empty search as the shortcut, not as text', async () => {
    await renderDashboard();

    expect(fireEvent.keyDown(searchField(), { key: '/' })).toBe(false);

    fireEvent.input(searchField(), { target: { value: 'a' } });
    expect(fireEvent.keyDown(searchField(), { key: '/' })).toBe(true);
  });

  it("clears the search with the button in the hint's place", async () => {
    await renderDashboard();
    expect(screen.queryByRole('button', { name: 'Clear search' })).toBeNull();

    fireEvent.input(searchField(), { target: { value: 'git' } });
    fireEvent.click(screen.getByRole('button', { name: 'Clear search' }));

    expect(rowTitles()).toHaveLength(3);
    expect((searchField() as HTMLInputElement).value).toBe('');
    expect(document.activeElement).toBe(searchField());
    expect(screen.queryByRole('button', { name: 'Clear search' })).toBeNull();
  });

  it('comes back to the search on "/" and locks on Ctrl+L', async () => {
    await renderDashboard();
    searchField().blur();

    fireEvent.keyDown(document.body, { key: '/' });
    expect(document.activeElement).toBe(searchField());

    fireEvent.keyDown(document.body, { key: 'l', ctrlKey: true });
    await waitFor(() => expect(onLocked).toHaveBeenCalledOnce());
    expect(calls.some((call) => call.cmd === 'lock_vault')).toBe(true);
  });
});
