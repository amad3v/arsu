import { createSignal } from 'solid-js';

import { IconifiedButton } from '@cpt/iconified-btn';
import { Modal } from '@cpt/modal';
import { isTouchUi } from '@cpt/touch-ui';

import { AddEntryForm } from './add-entry-form';

import type { ModalProps } from '@app-types/ui';
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

      <AddEntryModal
        open={open()}
        onClose={() => setOpen(false)}
        onSuccess={(id) => props.onSuccess(id)}
        finalFocusEl={() => trigger ?? null}
      />
    </>
  );
};

/** Read once: a touch screen has nothing to drop or paste an image with. */
const TOUCH_UI = isTouchUi();

export interface AddEntryModalProps extends AddEntryDialogProps {
  open: boolean;
  onClose: () => void;
  finalFocusEl?: ModalProps['finalFocusEl'];
}

/** The "Add entry" dialog alone, opened by its caller. */
export const AddEntryModal: Component<AddEntryModalProps> = (props) => (
  <Modal
    open={props.open}
    title={'Add entry'}
    description={
      TOUCH_UI
        ? 'Choose a screenshot of the QR code, or use its link or secret key.'
        : 'Paste or drop a screenshot of the QR code, or use its link or secret key.'
    }
    onClose={() => props.onClose()}
    finalFocusEl={props.finalFocusEl}
    wide
  >
    <AddEntryForm
      onAdded={(id) => {
        props.onClose();
        props.onSuccess(id);
      }}
    />
  </Modal>
);
