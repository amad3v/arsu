import { fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { appError, mockCommands } from '@cpt/testing/ipc';

import { scanQrInWorker } from './scan-qr';

import { AddEntryDialog } from '.';

import type { CommandHandler } from '@cpt/testing/ipc';

// jsdom has no workers: the scan is what the worker would reply.
vi.mock('./scan-qr', () => ({ scanQrInWorker: vi.fn() }));

const LINK = 'otpauth://totp/GitHub:alice?secret=JBSWY3DPEHPK3PXP';
const screenshot = new File(['png'], 'screenshot.png', { type: 'image/png' });

function renderDialog(commands: Partial<Record<string, CommandHandler>> = {}) {
  const calls = mockCommands(commands);
  const onSuccess = vi.fn();
  render(() => <AddEntryDialog onSuccess={onSuccess} />);
  return { calls, onSuccess };
}

async function openDialog(): Promise<HTMLElement> {
  fireEvent.click(screen.getByRole('button', { name: 'Add entry' }));
  return screen.findByRole('dialog', { name: 'Add entry' });
}

async function closeDialog() {
  fireEvent.click(screen.getByRole('button', { name: 'Close' }));
  await waitFor(() => {
    expect(screen.queryByRole('dialog')).toBeNull();
  });
}

function tab(name: string): HTMLElement {
  return screen.getByRole('tab', { name });
}

function linkField(): HTMLTextAreaElement {
  return screen.getByLabelText<HTMLTextAreaElement>('otpauth:// link');
}

function type(input: HTMLElement, value: string) {
  fireEvent.input(input, { target: { value } });
}

describe('AddEntryDialog', () => {
  afterEach(() => {
    vi.mocked(scanQrInWorker).mockReset();
  });

  it('shows which way of adding is selected', async () => {
    renderDialog();
    await openDialog();

    expect(tab('Link').getAttribute('aria-selected')).toBe('true');
    expect(tab('Link').hasAttribute('data-selected')).toBe(true);
    expect(tab('Manual').hasAttribute('data-selected')).toBe(false);

    fireEvent.click(tab('Manual'));
    await waitFor(() => {
      expect(tab('Manual').hasAttribute('data-selected')).toBe(true);
    });
  });

  describe('forgets everything typed when it closes', () => {
    it('after the user closes it', async () => {
      renderDialog();
      await openDialog();
      type(linkField(), LINK);
      fireEvent.click(tab('Manual'));
      type(await screen.findByLabelText('Secret key'), 'JBSWY3DPEHPK3PXP');

      await closeDialog();
      await openDialog();

      expect(linkField().value).toBe('');
      expect(screen.getByLabelText<HTMLInputElement>('Secret key').value).toBe('');
      expect(tab('Link').getAttribute('aria-selected')).toBe('true');
    });

    it('after an entry is added', async () => {
      const { onSuccess } = renderDialog({ add_entry_from_uri: () => 'new-id' });
      await openDialog();

      type(linkField(), LINK);
      fireEvent.click(screen.getByRole('button', { name: 'Add from link' }));

      await waitFor(() => {
        expect(onSuccess).toHaveBeenCalledWith('new-id');
      });
      await waitFor(() => {
        expect(screen.queryByRole('dialog')).toBeNull();
      });

      await openDialog();
      expect(linkField().value).toBe('');
    });
  });

  it('shows a rejected link under the link field', async () => {
    renderDialog({
      add_entry_from_uri: () => {
        throw appError('NotOtpauth', 'not an otpauth:// URI');
      },
    });
    await openDialog();

    type(linkField(), 'https://example.com');
    fireEvent.click(screen.getByRole('button', { name: 'Add from link' }));

    const error = await screen.findByText('not an otpauth:// URI');
    expect(linkField().getAttribute('aria-invalid')).toBe('true');
    expect(linkField().getAttribute('aria-describedby')?.split(' ')).toContain(error.id);
  });

  describe('paste', () => {
    it('scans a screenshot pasted on any tab and adds its account', async () => {
      vi.mocked(scanQrInWorker).mockResolvedValue({ status: 'found', text: LINK });
      const { calls, onSuccess } = renderDialog({ add_entry_from_uri: () => 'new-id' });
      await openDialog();
      fireEvent.click(tab('Manual'));

      fireEvent.paste(tab('Manual'), {
        clipboardData: { files: [screenshot], getData: () => '' },
      });

      await waitFor(() => {
        expect(onSuccess).toHaveBeenCalledWith('new-id');
      });
      expect(vi.mocked(scanQrInWorker).mock.calls[0][0]).toBe(screenshot);
      expect(calls).toEqual([{ cmd: 'add_entry_from_uri', args: { uri: LINK } }]);
    });

    it('adds the account in an image chosen in the native file dialog', async () => {
      vi.mocked(scanQrInWorker).mockResolvedValue({ status: 'found', text: LINK });
      const { calls, onSuccess } = renderDialog({
        pick_qr_image: () => btoa('png'),
        add_entry_from_uri: () => 'new-id',
      });
      await openDialog();
      fireEvent.click(tab('QR image'));

      fireEvent.click(await screen.findByRole('button', { name: 'Choose image…' }));

      await waitFor(() => {
        expect(onSuccess).toHaveBeenCalledWith('new-id');
      });
      const scanned = vi.mocked(scanQrInWorker).mock.calls[0][0];
      expect(await scanned.text()).toBe('png');
      expect(calls.map((call) => call.cmd)).toEqual(['pick_qr_image', 'add_entry_from_uri']);
    });

    it('scans nothing when the image file dialog is cancelled', async () => {
      const { calls } = renderDialog({ pick_qr_image: () => null });
      await openDialog();
      fireEvent.click(tab('QR image'));

      const choose = await screen.findByRole<HTMLButtonElement>('button', {
        name: 'Choose image…',
      });
      fireEvent.click(choose);

      await waitFor(() => {
        expect(choose.disabled).toBe(false);
      });
      expect(calls.map((call) => call.cmd)).toEqual(['pick_qr_image']);
      expect(scanQrInWorker).not.toHaveBeenCalled();
    });

    it('shows on the QR image tab why a pasted image did not work', async () => {
      vi.mocked(scanQrInWorker).mockResolvedValue({ status: 'not-found' });
      renderDialog();
      await openDialog();

      fireEvent.paste(linkField(), { clipboardData: { files: [screenshot], getData: () => '' } });

      expect((await screen.findByRole('alert')).textContent).toMatch(/^No QR code found/);
      expect(tab('QR image').getAttribute('aria-selected')).toBe('true');
    });

    it('puts a link pasted outside a text field into the link field', async () => {
      renderDialog();
      await openDialog();
      fireEvent.click(tab('Manual'));

      fireEvent.paste(tab('Manual'), {
        clipboardData: { files: [], getData: () => `${LINK}\n` },
      });

      await waitFor(() => {
        expect(tab('Link').getAttribute('aria-selected')).toBe('true');
      });
      expect(linkField().value).toBe(LINK);
      expect(document.activeElement).toBe(linkField());
    });
  });

  describe('manual entry', () => {
    let dialog: HTMLElement;

    async function openManualTab() {
      dialog = await openDialog();
      fireEvent.click(tab('Manual'));
      await waitFor(() => {
        expect(tab('Manual').getAttribute('aria-selected')).toBe('true');
      });
    }

    function submit() {
      fireEvent.click(within(dialog).getByRole('button', { name: /^Add (entry|anyway)$/ }));
    }

    it('shows what is missing next to each field, and sends nothing', async () => {
      const { calls } = renderDialog();
      await openManualTab();

      submit();

      const accountName = screen.getByLabelText('Account name');
      expect(
        await screen.findByText('Enter an account name, or at least an issuer.'),
      ).not.toBeNull();
      expect(screen.getByText('Enter the secret key.')).not.toBeNull();
      expect(accountName.getAttribute('aria-invalid')).toBe('true');
      expect(document.activeElement).toBe(accountName);
      expect(calls).toEqual([]);
    });

    it('rejects a bad period instead of replacing it with 30', async () => {
      const { calls } = renderDialog();
      await openManualTab();
      type(screen.getByLabelText('Account name'), 'alice');
      type(screen.getByLabelText('Secret key'), 'JBSWY3DPEHPK3PXP');

      // Typed as a user types: into the focused field, which then loses focus.
      const period = screen.getByLabelText('Period (seconds)');
      period.focus();
      type(period, '0');
      period.blur();

      expect(
        await screen.findByText('Use a whole number of seconds from 1 to 300.'),
      ).not.toBeNull();
      expect(calls).toEqual([]);
    });

    it('folds the code settings under a summary, and opens them to show an error', async () => {
      renderDialog({
        add_entry_manual: () => {
          throw appError('InvalidPeriod', 'the period must be 1 to 300 seconds');
        },
      });
      await openManualTab();
      const settings = within(dialog).getByRole('button', { name: /^Code settings/ });
      expect(settings.getAttribute('aria-expanded')).toBe('false');
      expect(settings.textContent).toContain('TOTP · SHA-1 · 6 digits · 30 s');

      type(screen.getByLabelText('Account name'), 'alice');
      type(screen.getByLabelText('Secret key'), 'JBSWY3DPEHPK3PXP');
      submit();

      await screen.findByText('the period must be 1 to 300 seconds');
      // The field takes focus once the settings have opened and shown it.
      await waitFor(() => {
        expect(document.activeElement).toBe(screen.getByLabelText('Period (seconds)'));
      });
      expect(settings.getAttribute('aria-expanded')).toBe('true');
    });

    it('adds a short secret only once the warning is confirmed', async () => {
      const { calls } = renderDialog({ add_entry_manual: () => 'new-id' });
      await openManualTab();
      type(screen.getByLabelText('Issuer (optional)'), 'gigo');
      type(screen.getByLabelText('Account name'), 'kilo');
      // "whatever" is valid base32, but only 40 bits.
      const secret = screen.getByLabelText('Secret key');
      type(secret, 'whatever');

      submit();
      const warning = await screen.findByText(/Check that you copied all of it/);
      expect(secret.getAttribute('aria-describedby')).toContain(warning.parentElement?.id);
      expect(secret.getAttribute('aria-invalid')).toBeNull();
      expect(calls).toEqual([]);

      // Editing the secret asks again.
      type(secret, 'whateverr');
      expect(within(dialog).queryByRole('button', { name: 'Add anyway' })).toBeNull();
      submit();
      const confirm = within(dialog).getByRole('button', { name: 'Add anyway' });
      expect(calls).toEqual([]);

      fireEvent.click(confirm);
      await waitFor(() => {
        expect(calls.map((call) => call.cmd)).toEqual(['add_entry_manual']);
      });
    });

    it("sends a valid entry, and shows the backend's rejection on its field", async () => {
      const { calls } = renderDialog({
        add_entry_manual: () => {
          throw appError('InvalidSecret', 'the secret is not valid base32');
        },
      });
      await openManualTab();
      type(screen.getByLabelText('Issuer (optional)'), 'GitHub');
      type(screen.getByLabelText('Account name'), 'alice');
      type(screen.getByLabelText('Secret key'), 'JBSWY3DPEHPK3PXPX');

      submit();

      await screen.findByText('the secret is not valid base32');
      expect(screen.getByLabelText('Secret key').getAttribute('aria-invalid')).toBe('true');
      expect(calls).toEqual([
        {
          cmd: 'add_entry_manual',
          args: {
            input: {
              issuer: 'GitHub',
              accountLabel: 'alice',
              secretBase32: 'JBSWY3DPEHPK3PXPX',
              algorithm: 'sha1',
              digits: 6,
              type: 'totp',
              period: 30,
            },
          },
        },
      ]);
    });
  });
});
