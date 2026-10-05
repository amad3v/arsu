import { createSignal, onCleanup, onMount, Show } from 'solid-js';

import { unlockVault, unlockWithBiometric } from '@api';
import { errorMessage, isAppError } from '@api/lib';
import { createBiometricOffer, NO_BIOMETRICS, readBiometricStatus } from '@cpt/biometric';
import { PasswordField } from '@cpt/password-field';
import { isTouchUi } from '@cpt/touch-ui';

import { AuthShell } from './auth-shell';
import { keepFocus } from './keep-focus';

import type { BiometricStatus } from '@app-types/api';
import type { Component } from 'solid-js';

/** Read once: only the touch interface waits for the biometric status before focusing. */
const TOUCH_UI = isTouchUi();

export interface UnlockScreenProps {
  /** The vault is unlocked. */
  onDone: () => void;
}

/**
 * The lock screen. With biometric unlock on (Android), it asks for the
 * fingerprint or face by itself, when it opens and whenever the app comes
 * back to the foreground, and keeps the password one tap away; otherwise
 * it is the password form, focused at once.
 */
export const UnlockScreen: Component<UnlockScreenProps> = (props) => {
  const [password, setPassword] = createSignal('');
  const [passwordError, setPasswordError] = createSignal<string | null>(null);
  const [error, setError] = createSignal<string | null>(null);
  const [notice, setNotice] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);
  // A fingerprint prompt is open. Kept apart from `busy`: the password stays
  // usable whatever becomes of the prompt.
  const [waiting, setWaiting] = createSignal(false);
  const [biometrics, setBiometrics] = createSignal<BiometricStatus>(NO_BIOMETRICS);
  // The user chose the password over the fingerprint, or it can't be used.
  const [passwordChosen, setPasswordChosen] = createSignal(false);
  const offer = createBiometricOffer();
  let input: HTMLInputElement | undefined;
  let root: HTMLDivElement | undefined;
  let unlockButton: HTMLButtonElement | undefined;

  const biometricReady = () => biometrics().enabled && biometrics().available;
  const showPassword = () => !biometricReady() || passwordChosen();

  // The fingerprint and the password can both succeed, one after the other.
  let done = false;
  function finish() {
    if (done) return;
    done = true;
    props.onDone();
  }

  function choosePassword() {
    setPasswordChosen(true);
    queueMicrotask(() => input?.focus());
  }

  async function tryBiometric() {
    if (waiting() || busy() || !biometricReady()) return;
    setWaiting(true);
    setError(null);
    try {
      await unlockWithBiometric();
      finish();
    } catch (err) {
      if (isAppError(err, 'BiometricCancelled')) {
        // The user can tap the fingerprint again, or use the password.
      } else if (isAppError(err, 'BiometricInvalidated')) {
        setBiometrics(await readBiometricStatus());
        setNotice(
          'Fingerprint and face unlock was turned off, because this phone’s fingerprints or faces changed. Unlock with your password, then turn it on again in Settings.',
        );
        choosePassword();
      } else if (isAppError(err, 'BiometricLockout')) {
        setError('Too many attempts. Unlock with your password.');
        choosePassword();
      } else if (
        isAppError(err, 'BiometricNotEnabled') ||
        isAppError(err, 'BiometricUnavailable')
      ) {
        setBiometrics(await readBiometricStatus());
        choosePassword();
      } else {
        setError(errorMessage(err));
      }
    } finally {
      setWaiting(false);
    }
  }

  /** Reads whether biometric unlock is on, and asks for it at once if so. */
  async function startBiometrics() {
    const status = await readBiometricStatus();
    setBiometrics(status);
    if (status.enabled && status.available) await tryBiometric();
    else if (TOUCH_UI) input?.focus();
  }

  onMount(() => {
    // This screen mounts again on every lock, the automatic one included, and
    // the user starts typing straight away. The `autofocus` attribute can't do
    // this: a document honours it only once. With biometric unlock on, the
    // prompt comes first instead, and the keyboard stays down: the touch
    // interface (the one with biometric unlock) focuses once it knows.
    if (!TOUCH_UI) input?.focus();
    void startBiometrics();

    // Back from another app, still locked: ask again.
    const onVisible = () => {
      if (document.visibilityState === 'visible' && !passwordChosen()) void tryBiometric();
    };
    document.addEventListener('visibilitychange', onVisible);
    onCleanup(() => document.removeEventListener('visibilitychange', onVisible));
  });

  // Typing the password is all there is to do here: no click takes the focus
  // from the field, but the Unlock button's. Not on a touch screen, where
  // focusing the field opens the keyboard: a tap anywhere else (the theme,
  // the fingerprint) must not.
  if (!TOUCH_UI)
    keepFocus({
      root: () => root,
      anchor: () => (showPassword() ? input : undefined),
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
      await offer.offer(password());
      finish();
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
      subtitle={
        showPassword() ? 'Enter your master password.' : 'Use your fingerprint or face to unlock.'
      }
    >
      <Show when={notice()}>
        {(message) => (
          <p class={'info mb-4'} role={'status'}>
            {message()}
          </p>
        )}
      </Show>

      <Show
        when={showPassword()}
        fallback={
          <div class={'flex flex-col gap-4 items-center'}>
            <button
              type={'button'}
              class={
                'text-primary pressable border border-border rounded-full bg-bg-card flex size-24 shadow-sm items-center justify-center active:bg-bg-pressed'
              }
              aria-label={'Unlock with your fingerprint or face'}
              aria-disabled={waiting() ? 'true' : undefined}
              onClick={() => void tryBiometric()}
            >
              <i class={'i-ph-fingerprint size-12'} aria-hidden={'true'} />
            </button>
            <p class={'subtle'}>
              {waiting() ? 'Waiting for your fingerprint or face…' : 'Tap to unlock'}
            </p>
            <Show when={error()}>
              {(message) => (
                <p class={'error w-full'} role={'alert'}>
                  {message()}
                </p>
              )}
            </Show>
            <button
              type={'button'}
              class={'text-base btn-ghost px-4 min-h-12'}
              onClick={() => choosePassword()}
            >
              {'Use password instead'}
            </button>
          </div>
        }
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

          <Show when={biometricReady()}>
            <button
              type={'button'}
              class={'text-base btn-ghost gap-2 min-h-12 w-full'}
              onClick={() => {
                setPasswordChosen(false);
                void tryBiometric();
              }}
            >
              <i class={'i-ph-fingerprint size-5'} aria-hidden={'true'} />
              {'Use fingerprint or face'}
            </button>
          </Show>
        </form>
      </Show>

      <offer.View />
    </AuthShell>
  );
};
