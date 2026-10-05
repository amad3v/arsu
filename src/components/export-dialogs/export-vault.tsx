import { createSignal, Show } from 'solid-js';

import { exportToAegisFile } from '@api';
import { errorMessage, isAppError } from '@api/lib';
import { checkNewPassword } from '@api/password-policy';
import { createFocusOnMount } from '@cpt/focus-on-mount';
import { IconifiedButton } from '@cpt/iconified-btn';
import { Modal } from '@cpt/modal';
import { NewPasswordFields } from '@cpt/new-password-fields';

import type { ModalProps } from '@app-types/ui';
import type { Component } from 'solid-js';

/** The "Export encrypted backup" button and its dialog. */
export const ExportVaultDialog: Component = () => {
  const [open, setOpen] = createSignal(false);
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

      <ExportVaultModal
        open={open()}
        onClose={() => setOpen(false)}
        finalFocusEl={() => trigger ?? null}
      />
    </>
  );
};

export interface ExportVaultModalProps {
  open: boolean;
  onClose: () => void;
  finalFocusEl?: ModalProps['finalFocusEl'];
  /**
   * Called once the backup is saved, for the caller to close the dialog and
   * show the result elsewhere (the touch interface's sheet, with
   * `ExportSaved`). Without it, the dialog shows the result itself.
   */
  onResult?: () => void;
}

/** The "Export encrypted backup" dialog alone, opened by its caller. */
export const ExportVaultModal: Component<ExportVaultModalProps> = (props) => {
  const [passwordInput, setPasswordInput] = createSignal<HTMLInputElement>();

  return (
    <Modal
      open={props.open}
      title={'Export encrypted backup'}
      description={'Saves every entry to a password-protected file that Aegis can import.'}
      onClose={() => props.onClose()}
      initialFocusEl={() => passwordInput() ?? null}
      finalFocusEl={props.finalFocusEl}
    >
      <ExportVaultForm
        onDone={() => props.onClose()}
        onResult={props.onResult}
        passwordRef={setPasswordInput}
      />
    </Modal>
  );
};

interface ExportVaultFormProps {
  onDone: () => void;
  onResult?: () => void;
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
      if (saved && props.onResult) props.onResult();
      else setOutcome(saved ? 'saved' : 'cancelled');
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

export interface ExportSavedProps {
  onDone: () => void;
  /**
   * Shown in the touch interface's sheet, which a tap outside it closes: a
   * headline with a key beside it, as an import's result has, and no Done
   * button.
   */
  inSheet?: boolean;
}

/** That the backup was saved, and that its password must be kept. */
export const ExportSaved: Component<ExportSavedProps> = (props) => {
  const focusMessage = createFocusOnMount();

  return (
    <Show
      when={props.inSheet}
      fallback={
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
      }
    >
      <div class={'space-y-4'}>
        <h3
          ref={focusMessage}
          tabIndex={-1}
          class={'text-lg text-text font-semibold flex gap-3 items-center focus:outline-none'}
        >
          <i class={'i-ph-key-bold text-primary shrink-0 size-6'} aria-hidden={'true'} />
          {'Backup saved'}
        </h3>
        <p class={'subtle'}>{"Keep its password safe: you'll need it to restore the backup."}</p>
      </div>
    </Show>
  );
};
