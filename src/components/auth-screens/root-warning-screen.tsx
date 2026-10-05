import { createSignal, Show } from 'solid-js';

import { answerRootWarning } from '@api';
import { errorMessage } from '@api/lib';
import { APP_NAME } from '@cpt/app-name';

import { AuthShell } from './auth-shell';

import type { Component } from 'solid-js';

export interface RootWarningScreenProps {
  /** The user accepted the risk; the app goes on to the vault. */
  onAccepted: () => void;
}

/**
 * Shown at start-up, before the vault, on a phone that looks rooted, until
 * the user accepts the risk. Leaving is the button that stands out;
 * accepting is the faded one, so that going on is a choice made on purpose.
 */
export const RootWarningScreen: Component<RootWarningScreenProps> = (props) => {
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  async function answer(accept: boolean) {
    if (busy()) return;
    setBusy(true);
    setError(null);
    try {
      await answerRootWarning(accept);
      if (accept) props.onAccepted();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <AuthShell title={'This phone is rooted'} subtitle={'Your codes are not safe on it.'}>
      <div class={'flex flex-col gap-4'}>
        <div class={'text-base warning space-y-2'} role={'alert'}>
          <p>
            {`An app with root access can read any other app’s memory and files: your codes, your vault while it is unlocked, and your master password as you type it. Android’s protections, which keep ${APP_NAME}’s data to itself, do not hold on a rooted phone.`}
          </p>
          <p>
            {`${APP_NAME} is provided as is, without warranty. If you go on, you do so at your own risk: its developer is not responsible for any loss or theft of your codes or accounts.`}
          </p>
        </div>

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
          onClick={() => void answer(false)}
        >
          {`Close ${APP_NAME}`}
        </button>
        <button
          type={'button'}
          class={'text-sm btn-ghost text-text-muted min-h-12 w-full'}
          disabled={busy()}
          onClick={() => void answer(true)}
        >
          {'I understand the risks, continue'}
        </button>
      </div>
    </AuthShell>
  );
};
