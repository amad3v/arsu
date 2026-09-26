import { createSignal, onCleanup, Show } from 'solid-js';

import { exportEntryQr } from '@api';
import { errorMessage, isAppError, svgToDataUri } from '@api/lib';
import { createDialogSubject } from '@cpt/dialog-subject';
import { entryLabel } from '@cpt/entry-label';
import { createFocusOnMount } from '@cpt/focus-on-mount';
import { Modal } from '@cpt/modal';
import { PasswordField } from '@cpt/password-field';

import type { EntrySummary } from '@app-types/api';
import type { Component } from 'solid-js';

export interface ExportEntryQrModalProps {
  /** The entry to export; null keeps the dialog closed. */
  entry: EntrySummary | null;
  onClose: () => void;
  /** The entry no longer exists (deleted meanwhile): the list needs a refresh. */
  onMissing: () => void;
  /** Where to return focus once the dialog closes: the row's ⋮ menu, or the search field if the row is gone. */
  finalFocusEl?: () => HTMLElement | null;
}

/** How long the QR code stays on screen before it hides itself. */
export const QR_VISIBLE_MS = 60_000;

/**
 * Shows one entry as a QR code, to move it to another authenticator app. The
 * QR code holds the entry's secret, so the dialog says so, asks for the
 * master password (checked by the backend) before showing it, and hides it
 * again after a minute.
 */
export const ExportEntryQrModal: Component<ExportEntryQrModalProps> = (props) => {
  const subject = createDialogSubject(() => props.entry);
  const [passwordInput, setPasswordInput] = createSignal<HTMLInputElement>();
  const title = () => {
    const entry = subject();
    return `Show ${entry === null ? 'entry' : entryLabel(entry)} as a QR code`;
  };

  return (
    <Modal
      open={props.entry !== null}
      title={title()}
      description={"Anyone who scans this QR code can generate this account's codes."}
      onClose={() => props.onClose()}
      initialFocusEl={() => passwordInput() ?? null}
      finalFocusEl={props.finalFocusEl}
    >
      <Show when={subject()}>
        {(entry) => (
          <ExportEntryQr
            entry={entry()}
            onMissing={() => props.onMissing()}
            passwordRef={setPasswordInput}
          />
        )}
      </Show>
    </Modal>
  );
};

interface ExportEntryQrProps {
  entry: EntrySummary;
  onMissing: () => void;
  passwordRef: (element: HTMLInputElement) => void;
}

/**
 * The dialog's content. It mounts and unmounts with the dialog, so the typed
 * password and the secret-bearing QR code are dropped when it closes.
 */
const ExportEntryQr: Component<ExportEntryQrProps> = (props) => {
  const [password, setPassword] = createSignal('');
  const [svg, setSvg] = createSignal<string | null>(null);
  const [passwordError, setPasswordError] = createSignal<string | null>(null);
  const [error, setError] = createSignal<string | null>(null);
  const [hidden, setHidden] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  let passwordInput: HTMLInputElement | undefined;
  let hideTimer: ReturnType<typeof setTimeout> | undefined;
  let mounted = true;

  onCleanup(() => {
    mounted = false;
    clearTimeout(hideTimer);
  });

  function show(image: string) {
    // Closed while the password was being checked: drop the secret unseen.
    if (!mounted) return;
    setSvg(image);
    setHidden(false);
    hideTimer = setTimeout(() => {
      setSvg(null);
      setHidden(true);
    }, QR_VISIBLE_MS);
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (busy() || password() === '') return;

    setBusy(true);
    setPasswordError(null);
    setError(null);
    try {
      show(await exportEntryQr(props.entry.id, password()));
      setPassword('');
    } catch (err) {
      if (isAppError(err, 'EntryNotFound')) {
        props.onMissing();
      } else if (isAppError(err, 'WrongPassword')) {
        setPasswordError('Incorrect password.');
        passwordInput?.focus();
        passwordInput?.select();
      } else {
        setError(errorMessage(err));
      }
    } finally {
      setBusy(false);
    }
  }

  return (
    <Show
      when={svg()}
      fallback={
        <form
          class={'space-y-4'}
          noValidate
          onSubmit={(event) => {
            void submit(event);
          }}
        >
          <div class={'error flex gap-2 items-start'}>
            <i class={'i-ph-warning-octagon mt-0.5 shrink-0 size-5'} aria-hidden={'true'} />
            <div class={'space-y-1'}>
              <p
                class={'font-medium'}
              >{`This QR code contains the secret key of ${entryLabel(props.entry)}.`}</p>
              <p>
                {
                  'Show it only to move this account to another authenticator app, where no one can see your screen.'
                }
              </p>
            </div>
          </div>

          <Show when={hidden()}>
            <p class={'info'} role={'status'}>
              {'The QR code was hidden after a minute. Enter your password to show it again.'}
            </p>
          </Show>

          <PasswordField
            ref={(element) => {
              passwordInput = element;
              props.passwordRef(element);
            }}
            label={'Master password'}
            description={'Needed each time you show a QR code.'}
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
            class={'btn-primary w-full'}
            disabled={busy() || password() === ''}
            type={'submit'}
          >
            {busy() ? 'Checking password…' : 'Show QR code'}
          </button>
        </form>
      }
    >
      {(image) => <QrCode svg={image()} entry={props.entry} />}
    </Show>
  );
};

interface QrCodeProps {
  svg: string;
  entry: EntrySummary;
}

const QrCode: Component<QrCodeProps> = (props) => {
  const focusFigure = createFocusOnMount();

  return (
    <figure
      ref={focusFigure}
      tabIndex={-1}
      class={'flex flex-col gap-4 items-center focus:outline-none'}
    >
      {/* White on purpose: a QR code must stay scannable in dark mode. */}
      <img
        src={svgToDataUri(props.svg)}
        alt={`QR code with the secret key of ${entryLabel(props.entry)}`}
        class={'p-2 border border-border rounded-lg bg-white size-64'}
      />
      <figcaption class={'subtle text-center'}>
        {
          'Scan it with your other authenticator app, then close this dialog. It hides itself after a minute.'
        }
      </figcaption>
    </figure>
  );
};
