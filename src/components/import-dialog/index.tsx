import { createSignal } from 'solid-js';

import { IconifiedButton } from '@cpt/iconified-btn';
import { Modal } from '@cpt/modal';

import { ImportFlow } from './import-flow';

import type { ImportSummary } from '@app-types/api';
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

      <Modal
        open={open()}
        title={'Import entries'}
        description={
          'Add the accounts from an Aegis vault export (.json) or a 2FAS backup (.2fas), encrypted or not.'
        }
        onClose={() => setOpen(false)}
        finalFocusEl={() => trigger ?? null}
      >
        <ImportFlow
          onImported={(summary) => props.onImported(summary)}
          onDone={() => setOpen(false)}
        />
      </Modal>
    </>
  );
};
