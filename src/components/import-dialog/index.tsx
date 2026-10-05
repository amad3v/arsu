import { createSignal } from 'solid-js';

import { IconifiedButton } from '@cpt/iconified-btn';
import { Modal } from '@cpt/modal';

import { ImportFlow } from './import-flow';

import type { ImportReport } from './import-report';
import type { ImportSummary } from '@app-types/api';
import type { ModalProps } from '@app-types/ui';
import type { Component } from 'solid-js';

export interface ImportDialogProps {
  /** Called as soon as entries are imported, while the dialog shows the result. */
  onImported: (summary: ImportSummary) => void;
}

/** The "Import entries" button and its dialog. */
export const ImportDialog: Component<ImportDialogProps> = (props) => {
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
        icon={'i-ph-upload'}
        label={'Import entries'}
      />

      <ImportModal
        open={open()}
        onClose={() => setOpen(false)}
        onImported={(summary) => props.onImported(summary)}
        finalFocusEl={() => trigger ?? null}
      />
    </>
  );
};

export interface ImportModalProps extends ImportDialogProps {
  open: boolean;
  onClose: () => void;
  finalFocusEl?: ModalProps['finalFocusEl'];
  /** Shows the result elsewhere, closing the dialog: see `ImportFlow`. */
  onResult?: (report: ImportReport) => void;
}

/** The "Import entries" dialog alone, opened by its caller. */
export const ImportModal: Component<ImportModalProps> = (props) => (
  <Modal
    open={props.open}
    title={'Import entries'}
    description={'Add the accounts from an Aegis vault export or a 2FAS backup, encrypted or not.'}
    onClose={() => props.onClose()}
    finalFocusEl={props.finalFocusEl}
  >
    <ImportFlow
      onImported={(summary) => props.onImported(summary)}
      onDone={() => props.onClose()}
      onResult={props.onResult}
    />
  </Modal>
);

export { ImportResult } from './import-result';
export type { ImportReport } from './import-report';
