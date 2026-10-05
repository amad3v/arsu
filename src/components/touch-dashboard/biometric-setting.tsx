import { createSignal, onMount, Show } from 'solid-js';

import { disableBiometricUnlock, enableBiometricUnlock } from '@api';
import { errorMessage, isAppError } from '@api/lib';
import {
  enableErrorMessage,
  NO_BIOMETRICS,
  readBiometricStatus,
  unavailableReason,
} from '@cpt/biometric';
import { Modal } from '@cpt/modal';
import { PasswordField } from '@cpt/password-field';
import { toaster } from '@cpt/toaster';

import type { BiometricStatus } from '@app-types/api';
import type { Component } from 'solid-js';

/**
 * The settings row that turns fingerprint and face unlock on and off: a
 * switch, or, when the phone can't use it, the reason why. Turning it on
 * asks for the master password, then Android asks for the fingerprint or
 * face; turning it off takes a tap.
 */
export const BiometricSetting: Component = () => {
  const [status, setStatus] = createSignal<BiometricStatus>(NO_BIOMETRICS);
  const [enabling, setEnabling] = createSignal(false);
  const [busy, setBusy] = createSignal(false);

  const refresh = async () => setStatus(await readBiometricStatus());
  onMount(() => void refresh());

  async function turnOff() {
    if (busy()) return;
    setBusy(true);
    try {
      await disableBiometricUnlock();
      toaster.create({ type: 'success', title: 'Fingerprint and face unlock is off' });
    } catch (err) {
      toaster.create({
        type: 'error',
        title: "Fingerprint and face unlock couldn't be turned off",
        description: errorMessage(err),
      });
    } finally {
      setBusy(false);
      await refresh();
    }
  }

  return (
    <Show when={status().supported}>
      <button
        type={'button'}
        role={'switch'}
        aria-checked={status().enabled}
        aria-disabled={busy() || !status().available ? 'true' : undefined}
        class={
          'px-4 py-3 text-left pressable flex gap-4 min-h-14 w-full items-center active:bg-bg-pressed'
        }
        onClick={() => {
          if (busy() || !status().available) return;
          if (status().enabled) void turnOff();
          else setEnabling(true);
        }}
      >
        <i class={'i-ph-fingerprint text-text-muted size-5'} aria-hidden={'true'} />
        <span class={'flex flex-1 flex-col min-w-0'}>
          <span>{'Unlock with fingerprint or face'}</span>
          <Show when={!status().available}>
            <span class={'text-xs text-text-muted'}>{unavailableReason(status())}</span>
          </Show>
        </span>
        <Switch on={status().enabled} />
      </button>

      <EnableBiometricModal
        open={enabling()}
        onClose={() => setEnabling(false)}
        onEnabled={() => {
          setEnabling(false);
          toaster.create({ type: 'success', title: 'Fingerprint and face unlock is on' });
          void refresh();
        }}
      />
    </Show>
  );
};

/** The switch's look; the row is the control. */
const Switch: Component<{ on: boolean }> = (props) => (
  <span
    aria-hidden={'true'}
    class={'p-0.5 rounded-full flex shrink-0 h-7 w-12 transition-colors items-center'}
    classList={{ 'bg-primary': props.on, 'bg-border-strong': !props.on }}
  >
    <span
      class={'rounded-full bg-white size-6 shadow-sm transition-transform'}
      classList={{ 'translate-x-5': props.on }}
    />
  </span>
);

interface EnableBiometricModalProps {
  open: boolean;
  onClose: () => void;
  onEnabled: () => void;
}

/** Turning it on: the master password first, then the fingerprint or face. */
const EnableBiometricModal: Component<EnableBiometricModalProps> = (props) => {
  const [input, setInput] = createSignal<HTMLInputElement>();

  return (
    <Modal
      open={props.open}
      title={'Unlock with fingerprint or face'}
      description={
        'Enter your master password to turn it on. Android then asks for your fingerprint or face.'
      }
      onClose={() => props.onClose()}
      initialFocusEl={() => input() ?? null}
    >
      <EnableForm passwordRef={setInput} onEnabled={() => props.onEnabled()} />
    </Modal>
  );
};

interface EnableFormProps {
  passwordRef: (element: HTMLInputElement) => void;
  onEnabled: () => void;
}

/** The modal's content; it lives only while the modal is open, and the password with it. */
const EnableForm: Component<EnableFormProps> = (props) => {
  const [password, setPassword] = createSignal('');
  const [passwordError, setPasswordError] = createSignal<string | null>(null);
  const [error, setError] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (busy() || password() === '') return;
    setBusy(true);
    setPasswordError(null);
    setError(null);
    try {
      await enableBiometricUnlock(password());
      props.onEnabled();
    } catch (err) {
      if (isAppError(err, 'WrongPassword')) setPasswordError('Incorrect password.');
      else setError(enableErrorMessage(err) ?? 'Cancelled. Nothing was changed.');
    } finally {
      setBusy(false);
    }
  }

  return (
    <form
      class={'space-y-4'}
      noValidate
      onSubmit={(event) => {
        void submit(event);
      }}
    >
      <PasswordField
        ref={props.passwordRef}
        label={'Master password'}
        value={password()}
        onValueChange={(value) => {
          setPassword(value);
          setPasswordError(null);
        }}
        error={passwordError()}
        autocomplete={'current-password'}
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
        class={'btn-primary text-base min-h-12 w-full'}
        disabled={busy() || password() === ''}
        type={'submit'}
      >
        {busy() ? 'Waiting for your fingerprint or face…' : 'Continue'}
      </button>
      <p class={'subtle'}>
        {
          'Your master password keeps working, and is needed again if this phone’s fingerprints or faces change.'
        }
      </p>
    </form>
  );
};
