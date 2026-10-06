import { createSignal, Show } from 'solid-js';

import { deleteEntry } from '@api';
import { errorMessage, isAppError } from '@api/lib';
import { createDialogSubject } from '@cpt/dialog-subject';
import { entryLabel } from '@cpt/entry-label';
import { Modal } from '@cpt/modal';
import { SheetModal } from '@cpt/sheet';
import { isTouchUi } from '@cpt/touch-ui';

import type { EntrySummary } from '@app-types/api';
import type { Component } from 'solid-js';

export interface DeleteEntryProps {
  /** The entry to delete; null keeps the dialog closed. */
  entry: EntrySummary | null;
  onClose: () => void;
  /** The entry is gone: deleted now, or already deleted elsewhere. */
  onDeleted: (entry: EntrySummary) => void;
  /** Where to return focus once the dialog closes: the row's ⋮ menu, or the search field once the row is gone. */
  finalFocusEl?: () => HTMLElement | null;
}

/**
 * Read once: the touch interface asks in a bottom sheet, which replaces the
 * entry's actions sheet, rather than in a full-screen page.
 */
const Frame = isTouchUi() ? SheetModal : Modal;

/**
 * Asks before deleting an entry. The app has no way to bring an entry back,
 * and without it the user may be locked out of the account, so the dialog
 * names the entry and says both. It cannot be closed while the delete runs.
 */
export const DeleteEntry: Component<DeleteEntryProps> = (props) => {
  const subject = createDialogSubject(() => props.entry);
  const [busy, setBusy] = createSignal(false);
  const [cancelButton, setCancelButton] = createSignal<HTMLButtonElement>();
  const title = () => {
    const entry = subject();
    return entry === null ? 'Delete entry?' : `Delete ${entryLabel(entry)}?`;
  };

  return (
    <Frame
      open={props.entry !== null}
      title={title()}
      description={
        "You won't be able to generate codes for this account in this app any more, and this can't be undone."
      }
      onClose={() => {
        if (!busy()) props.onClose();
      }}
      initialFocusEl={() => cancelButton() ?? null}
      finalFocusEl={props.finalFocusEl}
    >
      <Show when={subject()}>
        {(entry) => (
          <DeleteConfirmation
            entry={entry()}
            busy={busy()}
            onBusyChange={setBusy}
            onCancel={() => props.onClose()}
            onDeleted={() => props.onDeleted(entry())}
            cancelRef={setCancelButton}
          />
        )}
      </Show>
    </Frame>
  );
};

interface DeleteConfirmationProps {
  entry: EntrySummary;
  busy: boolean;
  onBusyChange: (busy: boolean) => void;
  onCancel: () => void;
  onDeleted: () => void;
  cancelRef: (element: HTMLButtonElement) => void;
}

/** The dialog's content. It mounts with each opening, so no error outlives it. */
const DeleteConfirmation: Component<DeleteConfirmationProps> = (props) => {
  const [error, setError] = createSignal<string | null>(null);

  async function confirm() {
    if (props.busy) return;
    props.onBusyChange(true);
    setError(null);
    try {
      await deleteEntry(props.entry.id);
      props.onDeleted();
    } catch (err) {
      if (isAppError(err, 'EntryNotFound')) {
        props.onDeleted();
      } else {
        setError(errorMessage(err));
      }
    } finally {
      props.onBusyChange(false);
    }
  }

  return (
    <div class={'space-y-4'}>
      <p class={'text-sm'}>
        {
          "Before you delete it, make sure you can still sign in to this account another way, such as another authenticator app or the account's recovery codes."
        }
      </p>

      <Show when={error()}>
        {(message) => (
          <p class={'error'} role={'alert'}>
            {message()}
          </p>
        )}
      </Show>

      <div class={'flex flex-wrap gap-2 justify-end'}>
        {/* The safe choice takes the initial focus, through the dialog's initialFocusEl. */}
        <button
          ref={props.cancelRef}
          class={'btn'}
          disabled={props.busy}
          onClick={() => props.onCancel()}
          type={'button'}
        >
          {'Cancel'}
        </button>
        <button
          class={'btn-danger'}
          disabled={props.busy}
          onClick={() => void confirm()}
          type={'button'}
        >
          {props.busy ? 'Deleting…' : 'Delete entry'}
        </button>
      </div>
    </div>
  );
};
