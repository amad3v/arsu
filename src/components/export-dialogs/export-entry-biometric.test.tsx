import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library';
import { describe, expect, it, vi } from 'vitest';

import { appError, mockCommands } from '@cpt/testing/ipc';

import { ExportEntryQrModal } from '.';

import type { BiometricStatus, EntrySummary } from '@app-types/api';
import type { CommandHandler } from '@cpt/testing/ipc';

// The touch interface (Android), where biometric unlock lives.
vi.mock('@cpt/touch-ui', () => ({ isTouchUi: () => true }));

const ON: BiometricStatus = {
  supported: true,
  available: true,
  reason: null,
  enabled: true,
};
const OFF: BiometricStatus = { ...ON, enabled: false };

const github: EntrySummary = {
  id: 'id-1',
  issuer: 'GitHub',
  accountLabel: 'alice',
  otpType: 'totp',
  digits: 6,
  period: 30,
};

const SVG = '<svg xmlns="http://www.w3.org/2000/svg"/>';

function renderModal(status: BiometricStatus, withBiometric: CommandHandler = () => SVG) {
  const calls = mockCommands({
    biometric_status: () => status,
    export_entry_qr_with_biometric: withBiometric,
    export_entry_qr: () => SVG,
  });
  render(() => <ExportEntryQrModal entry={github} onClose={vi.fn()} onMissing={vi.fn()} />);
  return calls;
}

const qrImage = () =>
  screen.findByRole('img', {
    name: 'QR code with the secret key of GitHub (alice)',
  });

describe('ExportEntryQrModal with biometric unlock', () => {
  it('shows the QR code after the fingerprint, with no password asked', async () => {
    const calls = renderModal(ON);

    fireEvent.click(await screen.findByRole('button', { name: 'Show QR code' }));

    expect(await qrImage()).toBeTruthy();
    expect(screen.queryByLabelText('Master password')).toBeNull();
    expect(calls.map((call) => call.cmd)).toEqual([
      'biometric_status',
      'export_entry_qr_with_biometric',
    ]);
    expect(calls[1]?.args).toEqual({ entryId: 'id-1' });
  });

  it('keeps the password a tap away', async () => {
    const calls = renderModal(ON);

    fireEvent.click(await screen.findByRole('button', { name: 'Use password instead' }));
    fireEvent.input(await screen.findByLabelText('Master password', { selector: 'input' }), {
      target: { value: 'master password' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Show QR code' }));

    expect(await qrImage()).toBeTruthy();
    expect(calls[calls.length - 1]?.cmd).toBe('export_entry_qr');
  });

  it('falls back to the password once biometric unlock was turned off', async () => {
    renderModal(ON, () => {
      throw appError('BiometricInvalidated');
    });

    fireEvent.click(await screen.findByRole('button', { name: 'Show QR code' }));

    expect(await screen.findByLabelText('Master password', { selector: 'input' })).toBeTruthy();
  });

  it('asks for the password when biometric unlock is off', async () => {
    renderModal(OFF);

    expect(await screen.findByLabelText('Master password', { selector: 'input' })).toBeTruthy();
    await waitFor(() => {
      expect(screen.queryByRole('button', { name: 'Use password instead' })).toBeNull();
    });
  });
});
