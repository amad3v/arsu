import { createSignal, Show } from 'solid-js';

import { exportToAegisFile } from '@api';
import { errorMessage, isAppError } from '@api/lib';
import { checkNewPassword } from '@api/password-policy';
import { createFocusOnMount } from '@cpt/focus-on-mount';
import { IconifiedButton } from '@cpt/iconified-btn';
import { Modal } from '@cpt/modal';
import { NewPasswordFields } from '@cpt/new-password-fields';

import type { Component } from 'solid-js';

/** The "Export encrypted backup" button and its dialog. */
export const ExportVaultDialog: Component = () => {
  const [open, setOpen] = createSignal(false);
  const [passwordInput, setPasswordInput] = createSignal<HTMLInputElement>();
  // Not reactive: read only once the dialog closes, to give it back the focus
  // it opened from.
  let trigger: HTMLButtonElement | undefined;

  return (
    <>
      <IconifiedButton
        class={'btn'}
        onClick={(event) => {
          trigger = event.currentTarget;
          setOpen(true);
        }}
        icon={'i-ph-download'}
        label={'Export encrypted backup'}
      />

      <Modal
        open={open()}
        title={'Export encrypted backup'}
        description={'Saves every entry to a password-protected file that Aegis can import.'}
        onClose={() => setOpen(false)}
        initialFocusEl={() => passwordInput() ?? null}
        finalFocusEl={() => trigger ?? null}
      >
        <ExportVaultForm onDone={() => setOpen(false)} passwordRef={setPasswordInput} />
      </Modal>
    </>
  );
};

interface ExportVaultFormProps {
  onDone: () => void;
  passwordRef: (element: HTMLInputElement) => void;
}

/** Whether the user saved the file or cancelled the save dialog. */
type ExportOutcome = 'saved' | 'cancelled';

/** The dialog's content. It lives only while the dialog is open, and the passwords with it. */
const ExportVaultForm: Component<ExportVaultFormProps> = (props) => {
  const [password, setPassword] = createSignal('');
  const [confirmation, setConfirmation] = createSignal('');
  const [passwordError, setPasswordError] = createSignal<string | null>(null);
  const [error, setError] = createSignal<string | null>(null);
  const [outcome, setOutcome] = createSignal<ExportOutcome | null>(null);
  const [busy, setBusy] = createSignal(false);
  const valid = () => checkNewPassword(password(), confirmation()).valid;

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (busy() || !valid()) return;

    setBusy(true);
    setPasswordError(null);
    setError(null);
    setOutcome(null);
    try {
      const saved = await exportToAegisFile(password());
      if (saved) {
        setPassword('');
        setConfirmation('');
      }
      setOutcome(saved ? 'saved' : 'cancelled');
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
    <Show
      when={outcome() === 'saved'}
      fallback={
        <form
          class={'space-y-4'}
          noValidate
          onSubmit={(event) => {
            void submit(event);
          }}
        >
          <div class={'info flex gap-2 items-start'}>
            <i class={'i-ph-info mt-0.5 shrink-0 size-4'} aria-hidden={'true'} />
            <p>
              {
                "You'll need this password to restore the backup. It can't be recovered, so keep it somewhere safe."
              }
            </p>
          </div>

          <NewPasswordFields
            passwordRef={props.passwordRef}
            label={'Backup password'}
            confirmLabel={'Confirm backup password'}
            password={password()}
            confirmation={confirmation()}
            onPasswordChange={(value) => {
              setPassword(value);
              setPasswordError(null);
            }}
            onConfirmationChange={setConfirmation}
            error={passwordError()}
          />

          <Show when={outcome() === 'cancelled'}>
            <p class={'subtle'} role={'status'}>
              {'No file was saved.'}
            </p>
          </Show>

          <Show when={error()}>
            {(message) => (
              <p class={'error'} role={'alert'}>
                {message()}
              </p>
            )}
          </Show>

          <div class={'flex flex-wrap gap-2 justify-end'}>
            <button class={'btn'} disabled={busy()} onClick={() => props.onDone()} type={'button'}>
              {'Cancel'}
            </button>
            <button class={'btn-primary'} disabled={busy() || !valid()} type={'submit'}>
              {busy() ? 'Saving…' : 'Save to…'}
            </button>
          </div>
        </form>
      }
    >
      <ExportSaved onDone={() => props.onDone()} />
    </Show>
  );
};

const ExportSaved: Component<{ onDone: () => void }> = (props) => {
  const focusMessage = createFocusOnMount();

  return (
    <div class={'space-y-4'}>
      <p
        ref={focusMessage}
        tabIndex={-1}
        class={'success flex gap-2 items-start focus:outline-none'}
      >
        <i class={'i-ph-check-circle mt-0.5 shrink-0 size-4'} aria-hidden={'true'} />
        {"Backup saved. Keep its password safe: you'll need it to restore the backup."}
      </p>
      <div class={'flex justify-end'}>
        <button class={'btn-primary'} onClick={() => props.onDone()} type={'button'}>
          {'Done'}
        </button>
      </div>
    </div>
  );
};
