import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library';
import { describe, expect, it, vi } from 'vitest';

import { appError, mockCommands } from '@cpt/testing/ipc';

import { ImportFlow } from './import-flow';

import type { ImportSummary, PickedFile } from '@app-types/api';
import type { CommandHandler } from '@cpt/testing/ipc';

const picked: PickedFile = { token: 'token-1', fileName: 'aegis-export.json', format: 'aegis' };

const summary: ImportSummary = {
  importedIds: ['id-1', 'id-2'],
  skipped: [{ label: 'Bank (bob)', reason: 'unsupported algorithm MD5' }],
  duplicates: [{ label: 'GitHub (alice)', reason: 'already in the vault' }],
};

function renderFlow(importFile: CommandHandler) {
  const calls = mockCommands({ pick_import_file: () => picked, import_file: importFile });
  const onImported = vi.fn();
  const onDone = vi.fn();
  render(() => <ImportFlow onImported={onImported} onDone={onDone} />);
  return { calls, onImported, onDone };
}

const chooseButton = () => screen.getByRole<HTMLButtonElement>('button', { name: /^Choose/ });

async function pickAegisFile() {
  fireEvent.click(chooseButton());
  await screen.findByText(picked.fileName);
}

function importButton(): HTMLButtonElement {
  return screen.getByRole<HTMLButtonElement>('button', { name: 'Import' });
}

function passwordInput(): HTMLInputElement {
  return screen.getByLabelText<HTMLInputElement>('File password');
}

function importCalls(calls: ReturnType<typeof renderFlow>['calls']) {
  return calls.filter((call) => call.cmd === 'import_file').map((call) => call.args);
}

describe('ImportFlow', () => {
  it('imports nothing until a file is picked', () => {
    renderFlow(() => summary);

    expect(importButton().disabled).toBe(true);
  });

  it('offers one file dialog for either backup, focused first', async () => {
    const { calls } = renderFlow(() => summary);

    // zag gives a dialog's initial focus to its [autofocus] element.
    expect(chooseButton().hasAttribute('autofocus')).toBe(true);
    expect(screen.getAllByRole('button')).toHaveLength(2); // Choose, and Import.

    await pickAegisFile();
    expect(calls.filter((call) => call.cmd === 'pick_import_file')).toHaveLength(1);
    expect(screen.getByText('Aegis vault export (.json)')).not.toBeNull();
    expect(chooseButton().textContent).toBe('Choose another backup…');
  });

  it('keeps the picked file and asks for the password when the file is encrypted', async () => {
    const { calls, onDone } = renderFlow((args) => {
      if (args === undefined || !('password' in args) || args.password === null) {
        throw appError('PasswordRequired');
      }
      return summary;
    });

    await pickAegisFile();
    fireEvent.click(importButton());

    expect(await screen.findByText('This file is encrypted. Enter its password.')).not.toBeNull();
    expect(document.activeElement).toBe(passwordInput());
    expect(screen.getByText(picked.fileName)).not.toBeNull();
    expect(onDone).not.toHaveBeenCalled();

    fireEvent.input(passwordInput(), { target: { value: 'file password' } });
    fireEvent.click(importButton());

    await screen.findByText('Imported 2 entries.');
    expect(importCalls(calls)).toEqual([
      { token: 'token-1', password: null },
      { token: 'token-1', password: 'file password' },
    ]);
  });

  it('lets the user retry a wrong password with the same file', async () => {
    let attempts = 0;
    const { calls } = renderFlow(() => {
      attempts += 1;
      if (attempts === 1) throw appError('WrongPasswordOrCorrupted');
      return summary;
    });

    await pickAegisFile();
    fireEvent.input(passwordInput(), { target: { value: 'typo' } });
    fireEvent.click(importButton());

    expect(await screen.findByText('Wrong password, or the file is damaged.')).not.toBeNull();
    const input = passwordInput();
    expect(input.getAttribute('aria-invalid')).toBe('true');
    expect(document.activeElement).toBe(input);
    expect([input.selectionStart, input.selectionEnd]).toEqual([0, 'typo'.length]);

    fireEvent.input(input, { target: { value: 'right' } });
    fireEvent.click(importButton());

    await screen.findByText('Imported 2 entries.');
    expect(importCalls(calls)).toEqual([
      { token: 'token-1', password: 'typo' },
      { token: 'token-1', password: 'right' },
    ]);
  });

  it('shows what was imported, skipped and already there, until dismissed', async () => {
    const { onImported, onDone } = renderFlow(() => summary);

    await pickAegisFile();
    fireEvent.click(importButton());

    expect(await screen.findByText('Imported 2 entries.')).not.toBeNull();
    expect(onImported).toHaveBeenCalledWith(summary);
    expect(screen.getByText('Bank (bob)')).not.toBeNull();
    expect(screen.getByText('unsupported algorithm MD5')).not.toBeNull();
    expect(screen.getByText('GitHub (alice)')).not.toBeNull();
    expect(screen.getByText('already in the vault')).not.toBeNull();
    expect(screen.getByText(/Keep the original backup/)).not.toBeNull();
    expect(onDone).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole('button', { name: 'Done' }));
    expect(onDone).toHaveBeenCalledOnce();
  });

  it('shows other errors in the dialog and keeps it open', async () => {
    const { onDone } = renderFlow(() => {
      throw appError('UnrecognizedFormat', 'not an Aegis vault');
    });

    await pickAegisFile();
    fireEvent.click(importButton());

    expect((await screen.findByRole('alert')).textContent).toBe('not an Aegis vault');
    expect(screen.getByText(picked.fileName)).not.toBeNull();
    expect(onDone).not.toHaveBeenCalled();
  });

  it('asks for the file again once its token has expired', async () => {
    renderFlow(() => {
      throw appError('NoPendingImport');
    });

    await pickAegisFile();
    fireEvent.click(importButton());

    await screen.findByRole('alert');
    await waitFor(() => {
      expect(screen.queryByText(picked.fileName)).toBeNull();
    });
    expect(importButton().disabled).toBe(true);
  });

  it('keeps the dialog as it was when the file dialog is cancelled', async () => {
    mockCommands({ pick_import_file: () => null });
    render(() => <ImportFlow onImported={() => undefined} onDone={() => undefined} />);

    fireEvent.click(chooseButton());

    await waitFor(() => {
      expect(chooseButton().disabled).toBe(false);
    });
    expect(importButton().disabled).toBe(true);
    expect(screen.queryByLabelText('File password')).toBeNull();
  });
});
