import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library';
import { describe, expect, it, vi } from 'vitest';

import { appError, mockCommands } from '@cpt/testing/ipc';

import { BiometricSetting } from './biometric-setting';

import type { BiometricStatus } from '@app-types/api';

vi.mock('@cpt/touch-ui', () => ({ isTouchUi: () => true }));

const OFF: BiometricStatus = { supported: true, available: true, reason: null, enabled: false };

const toggle = () => screen.findByRole('switch', { name: /Unlock with fingerprint or face/ });

describe('BiometricSetting', () => {
  it('turns it on with the master password', async () => {
    let status = OFF;
    const calls = mockCommands({
      biometric_status: () => status,
      enable_biometric_unlock: () => {
        status = { ...OFF, enabled: true };
        return null;
      },
    });
    render(() => <BiometricSetting />);
    const row = await toggle();
    expect(row.getAttribute('aria-checked')).toBe('false');

    fireEvent.click(row);
    const input = await screen.findByLabelText('Master password', { selector: 'input' });
    fireEvent.input(input, { target: { value: 'correct horse' } });
    fireEvent.click(screen.getByRole('button', { name: 'Continue' }));

    await waitFor(async () => expect((await toggle()).getAttribute('aria-checked')).toBe('true'));
    expect(calls.find((call) => call.cmd === 'enable_biometric_unlock')?.args).toEqual({
      masterPassword: 'correct horse',
    });
  });

  it('marks a wrong password on the field', async () => {
    mockCommands({
      biometric_status: () => OFF,
      enable_biometric_unlock: () => {
        throw appError('WrongPassword');
      },
    });
    render(() => <BiometricSetting />);
    fireEvent.click(await toggle());
    const input = await screen.findByLabelText('Master password', { selector: 'input' });
    fireEvent.input(input, { target: { value: 'wrong' } });
    fireEvent.click(screen.getByRole('button', { name: 'Continue' }));

    expect(await screen.findByText('Incorrect password.')).toBeTruthy();
  });

  it('turns it off with a tap', async () => {
    let status: BiometricStatus = { ...OFF, enabled: true };
    const calls = mockCommands({
      biometric_status: () => status,
      disable_biometric_unlock: () => {
        status = OFF;
        return null;
      },
    });
    render(() => <BiometricSetting />);
    fireEvent.click(await toggle());

    await waitFor(async () => expect((await toggle()).getAttribute('aria-checked')).toBe('false'));
    expect(calls.some((call) => call.cmd === 'disable_biometric_unlock')).toBe(true);
  });

  it('says why it cannot be used, and does not toggle', async () => {
    mockCommands({
      biometric_status: () => ({ ...OFF, available: false, reason: 'not-enrolled' }),
    });
    render(() => <BiometricSetting />);
    const row = await toggle();

    expect(screen.getByText(/Set up a fingerprint/)).toBeTruthy();
    fireEvent.click(row);
    expect(screen.queryByLabelText('Master password', { selector: 'input' })).toBeNull();
  });
});
