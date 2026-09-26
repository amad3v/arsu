import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library';
import { createSignal } from 'solid-js';
import { describe, expect, it, vi } from 'vitest';

import { appError, deferred, mockCommands } from '@cpt/testing/ipc';

import { DeleteEntry } from '.';

import type { EntrySummary } from '@app-types/api';
import type { CommandHandler } from '@cpt/testing/ipc';

const github: EntrySummary = {
  id: 'id-1',
  issuer: 'GitHub',
  accountLabel: 'alice',
  otpType: 'totp',
  digits: 6,
  period: 30,
};
const bank: EntrySummary = { ...github, id: 'id-2', issuer: null, accountLabel: 'bob@bank' };

function renderDialog(deleteEntry: CommandHandler) {
  const calls = mockCommands({ delete_entry: deleteEntry });
  const [entry, setEntry] = createSignal<EntrySummary | null>(github);
  const onClose = vi.fn(() => setEntry(null));
  const onDeleted = vi.fn(() => setEntry(null));
  render(() => <DeleteEntry entry={entry()} onClose={onClose} onDeleted={onDeleted} />);
  return { calls, setEntry, onClose, onDeleted };
}

function deleteButton(): HTMLButtonElement {
  return screen.getByRole<HTMLButtonElement>('button', { name: /^(Delete entry|Deleting…)$/ });
}

function cancelButton(): HTMLButtonElement {
  return screen.getByRole<HTMLButtonElement>('button', { name: 'Cancel' });
}

describe('DeleteEntry', () => {
  it('names the entry and says the deletion cannot be undone', async () => {
    renderDialog(() => null);

    const dialog = await screen.findByRole('dialog', { name: 'Delete GitHub (alice)?' });
    expect(dialog.textContent).toContain(
      "You won't be able to generate codes for this account in this app any more",
    );
    expect(dialog.textContent).toContain("can't be undone");
  });

  it('gives Cancel the initial focus through the Modal, not a native [autofocus]', async () => {
    renderDialog(() => null);
    await screen.findByRole('dialog');

    // A native [autofocus] attribute here raced Ark's own focus-trap
    // activation and dropped keyboard focus to <body> on close.
    // jsdom has no layout, so zag's own focusability check can't be exercised
    // here to assert the resulting focus directly (see modal.test.tsx).
    expect(cancelButton().hasAttribute('autofocus')).toBe(false);
  });

  it('deletes the entry and reports it', async () => {
    const { calls, onDeleted } = renderDialog(() => null);
    await screen.findByRole('dialog');

    fireEvent.click(deleteButton());

    await waitFor(() => {
      expect(onDeleted).toHaveBeenCalledWith(github);
    });
    expect(calls).toEqual([{ cmd: 'delete_entry', args: { entryId: 'id-1' } }]);
  });

  it('treats an entry deleted meanwhile as deleted', async () => {
    const { onDeleted } = renderDialog(() => {
      throw appError('EntryNotFound');
    });
    await screen.findByRole('dialog');

    fireEvent.click(deleteButton());

    await waitFor(() => {
      expect(onDeleted).toHaveBeenCalledWith(github);
    });
  });

  it("doesn't show one entry's error in the next entry's dialog", async () => {
    const { setEntry } = renderDialog(() => {
      throw appError('StorageIo', 'disk full');
    });
    await screen.findByRole('dialog');

    fireEvent.click(deleteButton());
    expect((await screen.findByRole('alert')).textContent).toBe('disk full');

    fireEvent.click(cancelButton());
    await waitFor(() => {
      expect(screen.queryByRole('dialog')).toBeNull();
    });

    setEntry(bank);
    await screen.findByRole('dialog', { name: 'Delete bob@bank?' });
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('cannot be cancelled or closed while the delete runs', async () => {
    const pending = deferred<null>();
    const { onClose, onDeleted } = renderDialog(() => pending.promise);
    const dialog = await screen.findByRole('dialog');

    fireEvent.click(deleteButton());

    await waitFor(() => {
      expect(cancelButton().disabled).toBe(true);
    });
    expect(deleteButton().textContent).toBe('Deleting…');
    fireEvent.keyDown(dialog, { key: 'Escape' });
    expect(onClose).not.toHaveBeenCalled();

    pending.resolve(null);
    await waitFor(() => {
      expect(onDeleted).toHaveBeenCalledOnce();
    });
  });
});
