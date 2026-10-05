// Biometric unlock in the interface: whether it can be offered, the words
// for why not, and the one-time offer to turn it on after the password has
// been typed (on creating the vault, or unlocking it).

import { createSignal, Show } from 'solid-js';

import { biometricStatus, enableBiometricUnlock } from '@api';
import { errorMessage, isAppError } from '@api/lib';
import { Sheet } from '@cpt/sheet';
import { isTouchUi } from '@cpt/touch-ui';

import type { BiometricStatus } from '@app-types/api';
import type { Component } from 'solid-js';

/** What the status is taken to be when it can't be read: biometric unlock is off. */
export const NO_BIOMETRICS: BiometricStatus = {
  supported: false,
  available: false,
  reason: null,
  enabled: false,
};

/**
 * The status, or "nothing to offer" if it can't be read. Never rejects. Only
 * the touch interface (Android) has biometric unlock: the desktop doesn't ask.
 */
export async function readBiometricStatus(): Promise<BiometricStatus> {
  if (!isTouchUi()) return NO_BIOMETRICS;
  try {
    return await biometricStatus();
  } catch {
    return NO_BIOMETRICS;
  }
}

/** Why biometric unlock can't be used now, in the user's terms. */
export function unavailableReason(status: BiometricStatus): string {
  switch (status.reason) {
    case 'not-enrolled':
      return 'Set up a fingerprint, or a secure face unlock, in Android settings first.';
    case 'no-hardware':
      return 'This phone has no fingerprint sensor or secure face unlock.';
    case 'update-required':
      return 'Android needs a security update before biometrics can be used.';
    default:
      return "Biometrics can't be used right now.";
  }
}

/** The user's answer to a failed enable, or null when nothing needs saying. */
export function enableErrorMessage(err: unknown): string | null {
  if (isAppError(err, 'BiometricCancelled')) return null;
  if (isAppError(err, 'BiometricLockout')) {
    return 'Too many attempts. Try again later, once Android lets biometrics be used again.';
  }
  if (isAppError(err, 'BiometricUnavailable')) {
    return "Biometrics can't be used right now. Check that a fingerprint or face is set up.";
  }
  return errorMessage(err);
}

const OFFER_DISMISSED_KEY = 'arsu.biometric-offer-dismissed';

/** Whether the user said "Not now" to the offer on this device. */
function offerDismissed(): boolean {
  try {
    return localStorage.getItem(OFFER_DISMISSED_KEY) === 'true';
  } catch {
    return false;
  }
}

function dismissOffer() {
  try {
    localStorage.setItem(OFFER_DISMISSED_KEY, 'true');
  } catch {
    // Not remembered: the offer comes back on the next unlock.
  }
}

export interface BiometricOffer {
  /**
   * Offers to turn biometric unlock on with `password`, just typed, if this
   * phone can and it isn't on yet, and the user hasn't declined before.
   * Resolves once the user has answered (or at once, with nothing to offer).
   */
  offer: (password: string) => Promise<void>;
  /** The offer's sheet: render it once, in the screen that offers. */
  View: Component;
}

/** The offer to turn biometric unlock on, right after the password was typed. */
export function createBiometricOffer(): BiometricOffer {
  const [open, setOpen] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  let pending: { password: string; done: () => void } | null = null;

  function finish() {
    setOpen(false);
    setError(null);
    pending?.done();
    pending = null;
  }

  async function turnOn() {
    if (pending === null || busy()) return;
    setBusy(true);
    setError(null);
    try {
      await enableBiometricUnlock(pending.password);
      finish();
    } catch (err) {
      const message = enableErrorMessage(err);
      if (message === null) finish();
      else setError(message);
    } finally {
      setBusy(false);
    }
  }

  return {
    offer: async (password) => {
      if (offerDismissed()) return;
      const status = await readBiometricStatus();
      if (!status.supported || !status.available || status.enabled) return;
      await new Promise<void>((done) => {
        pending = { password, done };
        setOpen(true);
      });
    },
    View: () => (
      <Sheet
        open={open()}
        title={'Unlock faster next time'}
        onClose={() => {
          if (!busy()) finish();
        }}
      >
        <div class={'px-5 pb-3 pt-1 flex flex-col gap-4'}>
          <div class={'flex gap-4 items-center'}>
            <span
              class={
                'text-primary rounded-full bg-info-bg flex shrink-0 size-14 items-center justify-center'
              }
            >
              <i class={'i-ph-fingerprint size-8'} aria-hidden={'true'} />
            </span>
            <p class={'text-base'}>
              {'Use your fingerprint or face to unlock, instead of typing your master password.'}
            </p>
          </div>
          <p class={'subtle'}>
            {
              'Your password still works, and is still needed if the phone’s fingerprints or faces change. You can turn this off in Settings.'
            }
          </p>
          <Show when={error()}>
            {(message) => (
              <p class={'error'} role={'alert'}>
                {message()}
              </p>
            )}
          </Show>
          <button
            type={'button'}
            class={'btn-primary text-base min-h-12 w-full'}
            disabled={busy()}
            onClick={() => void turnOn()}
          >
            {busy() ? 'Waiting for your fingerprint…' : 'Turn on'}
          </button>
          <button
            type={'button'}
            class={'text-base btn-ghost min-h-12 w-full'}
            disabled={busy()}
            onClick={() => {
              dismissOffer();
              finish();
            }}
          >
            {'Not now'}
          </button>
        </div>
      </Sheet>
    ),
  };
}
