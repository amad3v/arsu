import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { appError, deferred, mockCommands } from '@cpt/testing/ipc';

import { UnlockScreen } from '.';

import type { BiometricStatus } from '@app-types/api';

// The touch interface (Android), where biometric unlock lives.
vi.mock('@cpt/touch-ui', () => ({ isTouchUi: () => true }));

const ON: BiometricStatus = { supported: true, available: true, reason: null, enabled: true };
const OFF: BiometricStatus = { ...ON, enabled: false };

function passwordInput(): HTMLInputElement {
  return screen.getByLabelText('Master password', { selector: 'input' });
}

beforeEach(() => {
  localStorage.clear();
});

afterEach(() => {
  localStorage.clear();
});

describe('UnlockScreen with biometric unlock', () => {
  it('asks for the fingerprint at once, and unlocks', async () => {
    const calls = mockCommands({ biometric_status: () => ON, unlock_with_biometric: () => null });
    const onDone = vi.fn();
    render(() => <UnlockScreen onDone={onDone} />);

    await waitFor(() => expect(onDone).toHaveBeenCalledOnce());
    expect(calls.map((call) => call.cmd)).toEqual(['biometric_status', 'unlock_with_biometric']);
  });

  it('keeps the password usable while the fingerprint prompt goes unanswered', async () => {
    // The system's prompt can be held back (the phone locked) and never answer.
    const prompt = deferred<null>();
    const calls = mockCommands({
      biometric_status: () => ON,
      unlock_with_biometric: () => prompt.promise,
      unlock_vault: () => null,
    });
    const onDone = vi.fn();
    render(() => <UnlockScreen onDone={onDone} />);

    expect(await screen.findByText('Waiting for your fingerprint or face…')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Use password instead' }));
    fireEvent.input(passwordInput(), { target: { value: 'hunter2' } });
    const unlock = screen.getByRole('button', { name: 'Unlock' });
    expect(unlock).toHaveProperty('disabled', false);
    fireEvent.click(unlock);

    await waitFor(() => expect(onDone).toHaveBeenCalledOnce());
    expect(calls.map((call) => call.cmd)).toContain('unlock_vault');

    // The fingerprint passing after all doesn't unlock twice.
    prompt.resolve(null);
    await Promise.resolve();
    expect(onDone).toHaveBeenCalledOnce();
  });

  it('stays on the fingerprint after a cancel, with the password a tap away', async () => {
    mockCommands({
      biometric_status: () => ON,
      unlock_with_biometric: () => {
        throw appError('BiometricCancelled');
      },
    });
    const onDone = vi.fn();
    render(() => <UnlockScreen onDone={onDone} />);

    const fingerprint = await screen.findByRole('button', {
      name: 'Unlock with your fingerprint or face',
    });
    expect(fingerprint).toBeTruthy();
    expect(screen.queryByLabelText('Master password', { selector: 'input' })).toBeNull();

    fireEvent.click(screen.getByRole('button', { name: 'Use password instead' }));
    expect(passwordInput()).toBeTruthy();
    expect(onDone).not.toHaveBeenCalled();
  });

  it('says why when changed biometrics turned it off, and asks for the password', async () => {
    mockCommands({
      biometric_status: () => ON,
      unlock_with_biometric: () => {
        throw appError('BiometricInvalidated');
      },
    });
    render(() => <UnlockScreen onDone={() => undefined} />);

    expect(await screen.findByText(/fingerprints or faces changed/)).toBeTruthy();
    expect(passwordInput()).toBeTruthy();
  });

  it('offers biometric unlock after a password unlock, and turns it on with that password', async () => {
    const calls = mockCommands({
      biometric_status: () => OFF,
      unlock_vault: () => null,
      enable_biometric_unlock: () => null,
    });
    const onDone = vi.fn();
    render(() => <UnlockScreen onDone={onDone} />);
    await waitFor(() => expect(document.activeElement).toBe(passwordInput()));

    fireEvent.input(passwordInput(), { target: { value: 'correct horse' } });
    fireEvent.click(screen.getByRole('button', { name: 'Unlock' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Turn on' }));

    await waitFor(() => expect(onDone).toHaveBeenCalledOnce());
    expect(calls.find((call) => call.cmd === 'enable_biometric_unlock')?.args).toEqual({
      masterPassword: 'correct horse',
    });
  });

  it('does not offer again once declined', async () => {
    mockCommands({ biometric_status: () => OFF, unlock_vault: () => null });
    const first = vi.fn();
    const { unmount } = render(() => <UnlockScreen onDone={first} />);
    fireEvent.input(passwordInput(), { target: { value: 'correct horse' } });
    fireEvent.click(screen.getByRole('button', { name: 'Unlock' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Not now' }));
    await waitFor(() => expect(first).toHaveBeenCalledOnce());
    unmount();

    const second = vi.fn();
    render(() => <UnlockScreen onDone={second} />);
    fireEvent.input(passwordInput(), { target: { value: 'correct horse' } });
    fireEvent.click(screen.getByRole('button', { name: 'Unlock' }));
    await waitFor(() => expect(second).toHaveBeenCalledOnce());
    expect(screen.queryByRole('button', { name: 'Turn on' })).toBeNull();
  });

  it('leaves the keyboard down when something else on the screen is tapped', async () => {
    mockCommands({ biometric_status: () => OFF });
    render(() => <UnlockScreen onDone={() => undefined} />);
    await waitFor(() => expect(document.activeElement).toBe(passwordInput()));
    passwordInput().blur();

    fireEvent.mouseDown(screen.getByRole('heading', { name: 'Unlock vault' }));

    expect(document.activeElement).not.toBe(passwordInput());
  });
});
