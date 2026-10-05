import { createSignal, Show } from 'solid-js';

import { importFile, pickImportFile } from '@api';
import { errorMessage, isAppError } from '@api/lib';
import { PasswordField } from '@cpt/password-field';

import { IMPORT_FORMAT_LABEL } from './data';
import { importReport } from './import-report';
import { ImportResult } from './import-result';

import type { ImportReport } from './import-report';
import type { ImportSummary, PickedFile } from '@app-types/api';
import type { Component } from 'solid-js';

export interface ImportFlowProps {
  /** Called as soon as entries are imported, so the list refreshes behind the dialog. */
  onImported: (summary: ImportSummary) => void;
  /** The user dismissed the result. */
  onDone: () => void;
  /**
   * Takes the result to show it elsewhere (the touch interface's sheet),
   * instead of in the dialog. The dialog is then done with.
   */
  onResult?: (report: ImportReport) => void;
}

type Busy = 'picking' | 'importing' | null;

/**
 * The import dialog's content: pick a backup, give its password if it is
 * encrypted, import, and read the result. It lives only while the dialog is
 * open. An error leaves the dialog open with the picked file kept, so the
 * user retries with a password without picking the file again.
 */
export const ImportFlow: Component<ImportFlowProps> = (props) => {
  // Picked in the native dialog. Its path stays in Rust, behind the token.
  const [picked, setPicked] = createSignal<PickedFile | null>(null);
  const [password, setPassword] = createSignal('');
  const [passwordError, setPasswordError] = createSignal<string | null>(null);
  const [error, setError] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal<Busy>(null);
  const [report, setReport] = createSignal<ImportReport | null>(null);
  let passwordInput: HTMLInputElement | undefined;
  let importButton: HTMLButtonElement | undefined;

  async function choose() {
    if (busy() !== null) return;
    setBusy('picking');
    setError(null);
    try {
      const file = await pickImportFile();
      if (file === null) return; // The user cancelled the file dialog.

      setPicked(file);
      setPassword('');
      setPasswordError(null);
      importButton?.focus();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(null);
    }
  }

  function askForPassword(message: string) {
    setPasswordError(message);
    passwordInput?.focus();
    passwordInput?.select();
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    const current = picked();
    if (current === null || busy() !== null) return;

    setBusy('importing');
    setError(null);
    setPasswordError(null);
    try {
      const summary = await importFile(current.token, password() === '' ? null : password());
      setPassword('');
      props.onImported(summary);
      if (props.onResult) props.onResult(importReport(summary));
      else setReport(importReport(summary));
    } catch (err) {
      if (isAppError(err, 'PasswordRequired')) {
        askForPassword('This file is encrypted. Enter its password.');
      } else if (isAppError(err, 'WrongPasswordOrCorrupted')) {
        askForPassword('Wrong password, or the file is damaged.');
      } else if (isAppError(err, 'NoPendingImport')) {
        setPicked(null);
        setError('That file is no longer selected. Choose it again.');
      } else {
        setError(errorMessage(err));
      }
    } finally {
      setBusy(null);
    }
  }

  return (
    <Show
      when={report()}
      fallback={
        <form
          class={'space-y-4'}
          noValidate
          onSubmit={(event) => {
            void submit(event);
          }}
        >
          <div>
            <button
              // The dialog's first step takes the initial focus.
              autofocus
              class={'btn w-full'}
              disabled={busy() !== null}
              onClick={() => void choose()}
              type={'button'}
            >
              <i class={'i-ph-file-arrow-up size-4'} aria-hidden={'true'} />
              {picked() === null ? 'Choose backup…' : 'Choose another backup…'}
            </button>
            <p class={'subtle mt-2 min-h-5'} aria-live={'polite'}>
              {busy() === 'picking' ? 'Waiting for the file dialog…' : ''}
            </p>
          </div>

          <Show when={picked()}>
            {(current) => (
              <>
                <div class={'p-3 border border-info-border rounded-lg bg-info-bg min-w-0'}>
                  <p class={'text-sm text-text font-medium truncate'} title={current().fileName}>
                    {current().fileName}
                  </p>
                  <p class={'subtle mt-0.5'}>{IMPORT_FORMAT_LABEL[current().format]}</p>
                </div>

                <PasswordField
                  ref={(element) => {
                    passwordInput = element;
                  }}
                  label={'File password'}
                  description={'Only for an encrypted backup. Leave it empty otherwise.'}
                  value={password()}
                  onValueChange={(value) => {
                    setPassword(value);
                    setPasswordError(null);
                  }}
                  error={passwordError()}
                  autocomplete={'current-password'}
                  capsLockHint
                />
              </>
            )}
          </Show>

          <Show when={error()}>
            {(message) => (
              <p class={'error'} role={'alert'}>
                {message()}
              </p>
            )}
          </Show>

          <button
            ref={(element) => {
              importButton = element;
            }}
            class={'btn-primary w-full'}
            disabled={busy() !== null || picked() === null}
            type={'submit'}
          >
            {busy() === 'importing' ? 'Importing…' : 'Import'}
          </button>
        </form>
      }
    >
      {(done) => <ImportResult report={done()} onDone={() => props.onDone()} />}
    </Show>
  );
};
