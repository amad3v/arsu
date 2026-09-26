import { createSignal } from 'solid-js';

import { IconifiedButton } from '@cpt/iconified-btn';
import { Modal } from '@cpt/modal';

import { AddEntryForm } from './add-entry-form';

import type { Component } from 'solid-js';

export interface AddEntryDialogProps {
  /** Called with the new entry's id; the dialog has closed by then. */
  onSuccess: (id: string) => void;
}

/** The "Add entry" button and its dialog. */
export const AddEntryDialog: Component<AddEntryDialogProps> = (props) => {
  const [open, setOpen] = createSignal(false);
  // Not reactive: read only once the dialog closes, to give it back the focus
  // it opened from.
  let trigger: HTMLButtonElement | undefined;

  return (
    <>
      <IconifiedButton
        class={'btn-primary'}
        onClick={(event) => {
          trigger = event.currentTarget;
          setOpen(true);
        }}
        icon={'i-ph-file-plus'}
        label={'Add entry'}
      />

      <Modal
        open={open()}
        title={'Add entry'}
        description={'Paste or drop a screenshot of the QR code, or use its link or secret key.'}
        onClose={() => setOpen(false)}
        finalFocusEl={() => trigger ?? null}
        wide
      >
        <AddEntryForm
          onAdded={(id) => {
            setOpen(false);
            props.onSuccess(id);
          }}
        />
      </Modal>
    </>
  );
};
