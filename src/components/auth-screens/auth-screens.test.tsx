import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library';
import { createSignal, Show } from 'solid-js';
import { describe, expect, it, vi } from 'vitest';

import { appError, mockCommands } from '@cpt/testing/ipc';

import { CreateVaultScreen, UnlockScreen } from '.';

function passwordInput(label: string): HTMLInputElement {
  return screen.getByLabelText<HTMLInputElement>(label);
}

function type(input: HTMLInputElement, value: string) {
  fireEvent.input(input, { target: { value } });
}

describe('UnlockScreen', () => {
  it('focuses the password field each time it mounts, as after a lock', () => {
    mockCommands({});
    const [locked, setLocked] = createSignal(true);
    render(() => (
      <Show when={locked()}>
        <UnlockScreen onDone={() => undefined} />
      </Show>
    ));

    expect(document.activeElement).toBe(passwordInput('Master password'));

    setLocked(false);
    setLocked(true);
    expect(document.activeElement).toBe(passwordInput('Master password'));
  });

  it('offers nothing but what unlocks: the theme is chosen in the settings', () => {
    mockCommands({});
    render(() => <UnlockScreen onDone={() => undefined} />);

    expect(screen.queryByRole('button', { name: /Theme/ })).toBeNull();
  });

  it('focuses the field itself rather than through the autofocus attribute', () => {
    mockCommands({});
    render(() => <UnlockScreen onDone={() => undefined} />);

    expect(passwordInput('Master password').hasAttribute('autofocus')).toBe(false);
  });

  it('unlocks with the password and reports it', async () => {
    const calls = mockCommands({ unlock_vault: () => null });
    const onDone = vi.fn();
    render(() => <UnlockScreen onDone={onDone} />);

    type(passwordInput('Master password'), 'correct horse');
    fireEvent.click(screen.getByRole('button', { name: 'Unlock' }));

    await waitFor(() => {
      expect(onDone).toHaveBeenCalledOnce();
    });
    expect(calls).toEqual([{ cmd: 'unlock_vault', args: { masterPassword: 'correct horse' } }]);
  });

  it('marks a wrong password on the field and selects it for retyping', async () => {
    mockCommands({
      unlock_vault: () => {
        throw appError('WrongPassword');
      },
    });
    const onDone = vi.fn();
    render(() => <UnlockScreen onDone={onDone} />);
    const input = passwordInput('Master password');

    type(input, 'wrong');
    fireEvent.click(screen.getByRole('button', { name: 'Unlock' }));

    expect(await screen.findByText('Incorrect password.')).not.toBeNull();
    expect(input.getAttribute('aria-invalid')).toBe('true');
    expect(document.activeElement).toBe(input);
    expect([input.selectionStart, input.selectionEnd]).toEqual([0, 'wrong'.length]);
    expect(onDone).not.toHaveBeenCalled();
  });

  it('clears the error once the user types again', async () => {
    mockCommands({
      unlock_vault: () => {
        throw appError('WrongPassword');
      },
    });
    render(() => <UnlockScreen onDone={() => undefined} />);
    const input = passwordInput('Master password');

    type(input, 'wrong');
    fireEvent.click(screen.getByRole('button', { name: 'Unlock' }));
    await screen.findByText('Incorrect password.');

    type(input, 'wrong again');
    expect(screen.queryByText('Incorrect password.')).toBeNull();
  });

  it('explains when another window has the vault open', async () => {
    mockCommands({
      unlock_vault: () => {
        throw appError('VaultInUse');
      },
    });
    render(() => <UnlockScreen onDone={() => undefined} />);

    type(passwordInput('Master password'), 'pw');
    fireEvent.click(screen.getByRole('button', { name: 'Unlock' }));

    expect((await screen.findByRole('alert')).textContent).toContain('another window');
  });

  it('keeps the focus in the password field whatever is clicked, but Unlock', () => {
    mockCommands({});
    render(() => <UnlockScreen onDone={() => undefined} />);
    const input = passwordInput('Master password');
    const toggle = screen.getByRole('button', { name: /password/i });

    // A press on the background or on a control doesn't take the focus.
    expect(fireEvent.mouseDown(screen.getByRole('heading', { name: 'Unlock vault' }))).toBe(false);
    expect(fireEvent.mouseDown(toggle)).toBe(false);
    expect(document.activeElement).toBe(input);

    // Nor does a focus that follows a press (a menu handing it back as it closes)…
    toggle.focus();
    expect(document.activeElement).toBe(input);

    // …and a press brings it back from where the keyboard had taken it.
    fireEvent.keyDown(input, { key: 'Tab' });
    toggle.focus();
    expect(document.activeElement).toBe(toggle);
    fireEvent.mouseDown(screen.getByRole('heading', { name: 'Unlock vault' }));
    expect(document.activeElement).toBe(input);

    // The Unlock button can take it.
    type(input, 'password');
    expect(fireEvent.mouseDown(screen.getByRole('button', { name: 'Unlock' }))).toBe(true);
  });

  it('cannot be submitted empty', () => {
    mockCommands({});
    render(() => <UnlockScreen onDone={() => undefined} />);

    expect(screen.getByRole<HTMLButtonElement>('button', { name: 'Unlock' }).disabled).toBe(true);
  });
});

describe('CreateVaultScreen', () => {
  const strong = 'correct horse battery';

  function createButton(): HTMLButtonElement {
    return screen.getByRole<HTMLButtonElement>('button', { name: 'Create vault' });
  }

  it('focuses the master password field itself, not through the autofocus attribute', () => {
    mockCommands({});
    render(() => <CreateVaultScreen onDone={() => undefined} />);

    const input = passwordInput('Master password');
    expect(document.activeElement).toBe(input);
    expect(input.hasAttribute('autofocus')).toBe(false);
  });

  it('guides the user to a long enough password before submitting', () => {
    mockCommands({});
    render(() => <CreateVaultScreen onDone={() => undefined} />);
    const password = passwordInput('Master password');

    expect(screen.getByText('Use at least 12 characters.')).not.toBeNull();
    expect(createButton().disabled).toBe(true);

    type(password, 'short');
    expect(screen.getByText('Use at least 12 characters: 7 more to go.')).not.toBeNull();

    type(password, strong);
    expect(screen.getByText('At least 12 characters ✓')).not.toBeNull();
    expect(createButton().disabled).toBe(true);
  });

  it('shows whether the confirmation matches as it is typed', () => {
    mockCommands({});
    render(() => <CreateVaultScreen onDone={() => undefined} />);
    type(passwordInput('Master password'), strong);
    const confirmation = passwordInput('Confirm master password');

    type(confirmation, 'correct horse');
    expect(screen.getByText("The passwords don't match.")).not.toBeNull();
    expect(confirmation.getAttribute('aria-invalid')).toBe('true');

    type(confirmation, strong);
    expect(screen.getByText('The passwords match ✓')).not.toBeNull();
    expect(createButton().disabled).toBe(false);
  });

  it('relies on create_vault leaving the vault unlocked', async () => {
    const calls = mockCommands({ create_vault: () => null });
    const onDone = vi.fn();
    render(() => <CreateVaultScreen onDone={onDone} />);

    type(passwordInput('Master password'), strong);
    type(passwordInput('Confirm master password'), strong);
    fireEvent.click(createButton());

    await waitFor(() => {
      expect(onDone).toHaveBeenCalledOnce();
    });
    expect(calls).toEqual([{ cmd: 'create_vault', args: { masterPassword: strong } }]);
  });

  it("shows the backend's password rejection on the password field", async () => {
    mockCommands({
      create_vault: () => {
        throw appError('WeakPassword', 'the password must have at least 12 characters');
      },
    });
    render(() => <CreateVaultScreen onDone={() => undefined} />);

    type(passwordInput('Master password'), strong);
    type(passwordInput('Confirm master password'), strong);
    fireEvent.click(createButton());

    await screen.findByText('the password must have at least 12 characters');
    expect(passwordInput('Master password').getAttribute('aria-invalid')).toBe('true');
  });
});
