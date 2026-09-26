import { createSignal, onMount, Show } from 'solid-js';

import { createVault } from '@api';
import { errorMessage, isAppError } from '@api/lib';
import { checkNewPassword } from '@api/password-policy';
import { NewPasswordFields } from '@cpt/new-password-fields';

import { AuthShell } from './auth-shell';

import type { Component } from 'solid-js';

export interface CreateVaultScreenProps {
  /** The vault exists and, as create_vault leaves it, is unlocked. */
  onDone: () => void;
}

export const CreateVaultScreen: Component<CreateVaultScreenProps> = (props) => {
  const [password, setPassword] = createSignal('');
  const [confirmation, setConfirmation] = createSignal('');
  const [passwordError, setPasswordError] = createSignal<string | null>(null);
  const [error, setError] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);
  const valid = () => checkNewPassword(password(), confirmation()).valid;
  let input: HTMLInputElement | undefined;

  // Not a dialog, so no Modal to give it an initialFocusEl: it focuses itself,
  // the same way UnlockScreen does.
  onMount(() => input?.focus());

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (busy() || !valid()) return;

    setBusy(true);
    setPasswordError(null);
    setError(null);
    try {
      await createVault(password());
      props.onDone();
    } catch (err) {
      if (isAppError(err, 'WeakPassword')) {
        setPasswordError(errorMessage(err));
      } else {
        setError(errorMessage(err));
      }
    } finally {
      setBusy(false);
    }
  }

  return (
    <AuthShell
      title={'Create vault'}
      subtitle={
        'Your master password encrypts your accounts on this computer. If you forget it, there is no way to recover them.'
      }
    >
      <form
        class={'space-y-4'}
        noValidate
        onSubmit={(event) => {
          void submit(event);
        }}
      >
        <NewPasswordFields
          passwordRef={(element) => {
            input = element;
          }}
          label={'Master password'}
          confirmLabel={'Confirm master password'}
          password={password()}
          confirmation={confirmation()}
          onPasswordChange={(value) => {
            setPassword(value);
            setPasswordError(null);
          }}
          onConfirmationChange={setConfirmation}
          error={passwordError()}
        />

        <Show when={error()}>
          {(message) => (
            <p class={'error'} role={'alert'}>
              {message()}
            </p>
          )}
        </Show>

        <button class={'btn-primary w-full'} disabled={busy() || !valid()} type={'submit'}>
          {busy() ? 'Creating vault…' : 'Create vault'}
        </button>

        <p class={'subtle'}>{'Creating the vault takes about a second.'}</p>
      </form>
    </AuthShell>
  );
};
