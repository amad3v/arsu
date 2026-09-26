import { createSignal, onMount, Show } from 'solid-js';

import { unlockVault } from '@api';
import { errorMessage, isAppError } from '@api/lib';
import { PasswordField } from '@cpt/password-field';

import { AuthShell } from './auth-shell';
import { keepFocus } from './keep-focus';

import type { Component } from 'solid-js';

export interface UnlockScreenProps {
  /** The vault is unlocked. */
  onDone: () => void;
}

export const UnlockScreen: Component<UnlockScreenProps> = (props) => {
  const [password, setPassword] = createSignal('');
  const [passwordError, setPasswordError] = createSignal<string | null>(null);
  const [error, setError] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);
  let input: HTMLInputElement | undefined;
  let root: HTMLDivElement | undefined;
  let unlockButton: HTMLButtonElement | undefined;

  // This screen mounts again on every lock, the automatic one included, and
  // the user starts typing straight away. The `autofocus` attribute can't do
  // this: a document honours it only once.
  onMount(() => input?.focus());

  // Typing the password is all there is to do here: no click takes the focus
  // from the field, but the Unlock button's.
  keepFocus({
    root: () => root,
    anchor: () => input,
    allow: (target) => unlockButton?.contains(target) === true,
  });

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (busy() || password() === '') return;

    setBusy(true);
    setPasswordError(null);
    setError(null);
    try {
      await unlockVault(password());
      props.onDone();
    } catch (err) {
      if (isAppError(err, 'WrongPassword')) {
        setPasswordError('Incorrect password.');
        input?.focus();
        input?.select();
      } else if (isAppError(err, 'VaultInUse')) {
        setError('The vault is open in another window of this app. Close it, then unlock again.');
      } else {
        setError(errorMessage(err));
      }
    } finally {
      setBusy(false);
    }
  }

  return (
    <AuthShell
      ref={(element) => {
        root = element;
      }}
      title={'Unlock vault'}
      subtitle={'Enter your master password.'}
    >
      <form
        class={'space-y-4'}
        noValidate
        onSubmit={(event) => {
          void submit(event);
        }}
      >
        <PasswordField
          ref={(element) => {
            input = element;
          }}
          label={'Master password'}
          value={password()}
          onValueChange={(value) => {
            setPassword(value);
            setPasswordError(null);
          }}
          error={passwordError()}
          autocomplete={'current-password'}
          capsLockHint
          required
        />

        <Show when={error()}>
          {(message) => (
            <p class={'error'} role={'alert'}>
              {message()}
            </p>
          )}
        </Show>

        <button
          ref={(element) => {
            unlockButton = element;
          }}
          class={'btn-primary w-full'}
          disabled={busy() || password() === ''}
          type={'submit'}
        >
          {busy() ? 'Unlocking…' : 'Unlock'}
        </button>
      </form>
    </AuthShell>
  );
};
