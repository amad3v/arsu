import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library';
import { createSignal } from 'solid-js';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { appError, deferred, mockCommands } from '@cpt/testing/ipc';

import {
  ExportEntryQrModal,
  ExportSaved,
  ExportVaultDialog,
  ExportVaultModal,
  QR_VISIBLE_MS,
} from '.';

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

const SVG = '<svg xmlns="http://www.w3.org/2000/svg"/>';

function type(input: HTMLElement, value: string) {
  fireEvent.input(input, { target: { value } });
}

describe('ExportEntryQrModal', () => {
  function renderModal(exportEntryQr: CommandHandler) {
    const calls = mockCommands({ export_entry_qr: exportEntryQr });
    const [entry, setEntry] = createSignal<EntrySummary | null>(github);
    const onMissing = vi.fn();
    render(() => (
      <ExportEntryQrModal entry={entry()} onClose={() => setEntry(null)} onMissing={onMissing} />
    ));
    return { calls, setEntry, onMissing };
  }

  async function showQr() {
    type(await screen.findByLabelText('Master password'), 'master password');
    fireEvent.click(screen.getByRole('button', { name: 'Show QR code' }));
  }

  afterEach(() => {
    vi.useRealTimers();
  });

  it('names the entry and warns that its QR code is a secret, before showing anything', async () => {
    renderModal(() => SVG);

    const dialog = await screen.findByRole('dialog', { name: 'Show GitHub (alice) as a QR code' });
    expect(dialog.textContent).toContain('Anyone who scans this QR code can generate');
    expect(dialog.textContent).toContain('This QR code contains the secret key of GitHub (alice).');
    expect(screen.queryByRole('img')).toBeNull();
  });

  it('shows the QR code once the backend accepts the master password', async () => {
    const { calls } = renderModal(() => SVG);

    await showQr();

    const image = await screen.findByRole<HTMLImageElement>('img', {
      name: 'QR code with the secret key of GitHub (alice)',
    });
    expect(image.src).toMatch(/^data:image\/svg\+xml/);
    expect(calls).toEqual([
      { cmd: 'export_entry_qr', args: { entryId: 'id-1', masterPassword: 'master password' } },
    ]);
  });

  it('keeps the QR code hidden after a wrong password', async () => {
    renderModal(() => {
      throw appError('WrongPassword');
    });

    await showQr();

    expect(await screen.findByText('Incorrect password.')).not.toBeNull();
    expect(screen.getByLabelText('Master password').getAttribute('aria-invalid')).toBe('true');
    expect(screen.queryByRole('img')).toBeNull();
  });

  it('drops the QR code when closed: reopening asks for the password again', async () => {
    const { setEntry } = renderModal(() => SVG);
    await showQr();
    await screen.findByRole('img');

    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    await waitFor(() => {
      expect(screen.queryByRole('dialog')).toBeNull();
    });
    expect(document.querySelector('img')).toBeNull();

    setEntry(github);
    const input = await screen.findByLabelText<HTMLInputElement>('Master password');
    expect(input.value).toBe('');
    expect(screen.queryByRole('img')).toBeNull();
  });

  it('drops a QR code that arrives after the dialog closed', async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
    const pending = deferred<string>();
    renderModal(() => pending.promise);
    await showQr();

    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    await waitFor(() => {
      expect(screen.queryByRole('dialog')).toBeNull();
    });
    const timers = vi.getTimerCount();
    pending.resolve(SVG);
    await vi.advanceTimersByTimeAsync(0);

    // Nothing kept it: no hide timer was armed for a QR code no one sees.
    expect(vi.getTimerCount()).toBe(timers);
  });

  it('hides the QR code after a minute', async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
    renderModal(() => SVG);
    await showQr();
    await screen.findByRole('img');

    vi.advanceTimersByTime(QR_VISIBLE_MS);

    expect(screen.queryByRole('img')).toBeNull();
    expect(screen.getByText(/hidden after a minute/)).not.toBeNull();
  });

  it('reports an entry deleted meanwhile', async () => {
    const { onMissing } = renderModal(() => {
      throw appError('EntryNotFound');
    });

    await showQr();

    await waitFor(() => {
      expect(onMissing).toHaveBeenCalledOnce();
    });
  });
});

describe('ExportVaultDialog', () => {
  const strong = 'correct horse battery';

  async function openDialog(exportToAegisFile: CommandHandler) {
    const calls = mockCommands({ export_to_aegis_file: exportToAegisFile });
    render(() => <ExportVaultDialog />);
    fireEvent.click(screen.getByRole('button', { name: 'Export encrypted backup' }));
    await screen.findByRole('dialog');
    return calls;
  }

  function saveButton(): HTMLButtonElement {
    return screen.getByRole<HTMLButtonElement>('button', { name: 'Save to…' });
  }

  function fillPasswords(password: string, confirmation: string) {
    type(screen.getByLabelText('Backup password'), password);
    type(screen.getByLabelText('Confirm backup password'), confirmation);
  }

  it('warns that the password cannot be recovered', async () => {
    await openDialog(() => true);

    expect(screen.getByRole('dialog').textContent).toContain("It can't be recovered");
  });

  it('needs a long enough password, typed twice', async () => {
    await openDialog(() => true);

    fillPasswords('short', 'short');
    expect(saveButton().disabled).toBe(true);
    expect(screen.getByText('Use at least 12 characters: 7 more to go.')).not.toBeNull();

    fillPasswords(strong, 'correct horse');
    expect(saveButton().disabled).toBe(true);
    expect(screen.getByText("The passwords don't match.")).not.toBeNull();

    fillPasswords(strong, strong);
    expect(saveButton().disabled).toBe(false);
  });

  it('confirms a saved backup', async () => {
    const calls = await openDialog(() => true);

    fillPasswords(strong, strong);
    fireEvent.click(saveButton());

    const message = await screen.findByText(/^Backup saved\./);
    expect(document.activeElement).toBe(message);
    expect(calls).toEqual([{ cmd: 'export_to_aegis_file', args: { password: strong } }]);
  });

  it('says nothing was saved when the save dialog is cancelled, and keeps the form', async () => {
    await openDialog(() => false);

    fillPasswords(strong, strong);
    fireEvent.click(saveButton());

    expect((await screen.findByRole('status')).textContent).toBe('No file was saved.');
    expect(saveButton().disabled).toBe(false);
  });

  it("shows the backend's password rejection on the password field", async () => {
    await openDialog(() => {
      throw appError('WeakPassword', 'the password must have at least 12 characters');
    });

    fillPasswords(strong, strong);
    fireEvent.click(saveButton());

    await screen.findByText('the password must have at least 12 characters');
    expect(screen.getByLabelText('Backup password').getAttribute('aria-invalid')).toBe('true');
  });
});

describe('ExportVaultModal with onResult (the touch interface)', () => {
  it('hands a saved backup to its caller instead of showing it, but keeps a cancel', async () => {
    const strong = 'correct horse battery';
    const onResult = vi.fn();
    let saved = false;
    mockCommands({ export_to_aegis_file: () => saved });
    render(() => <ExportVaultModal open onClose={vi.fn()} onResult={onResult} />);
    await screen.findByRole('dialog');
    type(screen.getByLabelText('Backup password'), strong);
    type(screen.getByLabelText('Confirm backup password'), strong);
    const save = screen.getByRole('button', { name: 'Save to…' });

    fireEvent.click(save);
    expect((await screen.findByRole('status')).textContent).toBe('No file was saved.');
    expect(onResult).not.toHaveBeenCalled();

    saved = true;
    fireEvent.click(save);
    await waitFor(() => expect(onResult).toHaveBeenCalledOnce());
    expect(screen.queryByText(/Backup saved/)).toBeNull();
  });
});

describe('ExportSaved', () => {
  it('heads the sheet with the result, focused, and leaves closing to the sheet', () => {
    render(() => <ExportSaved onDone={vi.fn()} inSheet />);

    const headline = screen.getByRole('heading', { name: 'Backup saved' });
    expect(document.activeElement).toBe(headline);
    expect(screen.getByText(/Keep its password safe/)).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Done' })).toBeNull();
  });
});
