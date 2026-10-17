import { Show } from 'solid-js';

import { countLabel, fileSize } from '@cpt/format';
import { Modal } from '@cpt/modal';
import { scrollFade } from '@cpt/scroll-fade';

import type { DiagnosticReportPreview } from '@app-types/api';
import type { Component } from 'solid-js';

export interface DiagnosticReportDialogProps {
  /** The report to show; the dialog is open while there is one. */
  preview: DiagnosticReportPreview | null;
  /** Whether it is being saved: the buttons wait. */
  saving: boolean;
  onSave: () => void;
  onClose: () => void;
}

/**
 * The diagnostic report, shown whole before it is saved: what the app
 * and the device are, then its latest records, exactly as the file holds
 * them. Nothing leaves the device unless the user saves it and shares it.
 */
export const DiagnosticReportDialog: Component<DiagnosticReportDialogProps> = (props) => (
  <Modal
    open={props.preview !== null}
    title={'Diagnostic report'}
    description={
      'This is what the report holds. It leaves this device only if you save it and share the file.'
    }
    wide
    page
    onClose={() => props.onClose()}
  >
    <Show when={props.preview}>
      {(preview) => (
        <div class={'flex flex-col gap-4'}>
          <p class={'subtle text-sm break-all'}>
            {`${preview().fileName} · ${fileSize(preview().bytes)} · ${countLabel(preview().records, 'record', 'records')}`}
          </p>

          <section class={'flex flex-col gap-2'}>
            <h3 class={'text-sm font-medium'}>{'The app and this device'}</h3>
            <ReportText text={preview().environment} />
          </section>

          <section class={'flex flex-col gap-2'}>
            <h3 class={'text-sm font-medium'}>
              {preview().latestRecords < preview().records
                ? `The log: its latest ${preview().latestRecords} records`
                : 'The log'}
            </h3>
            <Show
              when={preview().records > 0}
              fallback={
                <p class={'subtle text-sm'}>
                  {'Nothing has gone wrong on this device yet: the log is empty.'}
                </p>
              }
            >
              <ReportText text={preview().latest} />
            </Show>
          </section>

          <div class={'flex flex-wrap gap-2 justify-end'}>
            <button
              type={'button'}
              class={'btn'}
              disabled={props.saving}
              onClick={() => props.onClose()}
            >
              {'Cancel'}
            </button>
            <button
              type={'button'}
              class={'btn-primary'}
              disabled={props.saving}
              onClick={() => props.onSave()}
            >
              <i class={'i-ph-file-zip size-4'} aria-hidden={'true'} />
              {'Save report'}
            </button>
          </div>
        </div>
      )}
    </Show>
  </Modal>
);

/** A part of the report, as text: it scrolls in its box, wrapped, selectable. */
const ReportText: Component<{ text: string }> = (props) => (
  <div class={'border border-border rounded-md bg-bg-input flex flex-col'}>
    <pre
      ref={scrollFade}
      class={
        'scroll-fade text-xs text-text-muted font-mono p-3 max-h-60 select-text whitespace-pre-wrap break-all overflow-y-auto'
      }
    >
      {props.text}
    </pre>
  </div>
);
