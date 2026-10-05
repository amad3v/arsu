import { createSignal, onMount, Show } from 'solid-js';

import { createVault } from '@api';
import { errorMessage, isAppError } from '@api/lib';
import { checkNewPassword } from '@api/password-policy';
import { createBiometricOffer } from '@cpt/biometric';
import { NewPasswordFields } from '@cpt/new-password-fields';
import { isTouchUi } from '@cpt/touch-ui';

import { AuthShell } from './auth-shell';

import type { Component } from 'solid-js';

/** Read once: the touch interface runs on a phone. */
const TOUCH_UI = isTouchUi();

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
  // On a phone that can, the next unlock can be a fingerprint instead.
  const offer = createBiometricOffer();
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
      await offer.offer(password());
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
      subtitle={`Your master password encrypts your accounts on this ${TOUCH_UI ? 'phone' : 'computer'}. If you forget it, there is no way to recover them.`}
    >
      <form
        class={'space-y-4'}
        noValidate
        onSubmit={(event) => {
          void submit(event);
        }}
      >
        {/*
          A phone's app data goes with the app: uninstalling it, or clearing its
          storage, deletes the vault, and Android's cloud backup leaves it out.
        */}
        <Show when={TOUCH_UI}>
          <div class={'warning border-2 flex gap-3 items-start'} role={'note'}>
            <i class={'i-ph-warning-fill mt-0.5 shrink-0 size-5'} aria-hidden={'true'} />
            <p>
              <strong class={'font-semibold'}>{'Uninstalling Arsu deletes your vault'}</strong>
              {
                ', with every account in it, and so does clearing its storage. Before you do, export an encrypted backup with the + button.'
              }
            </p>
          </div>
        </Show>

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

      <offer.View />
    </AuthShell>
  );
};
