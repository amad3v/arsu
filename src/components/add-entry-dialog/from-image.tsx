import { FileUpload } from '@ark-ui/solid/file-upload';
import { Show } from 'solid-js';

import { QR_IMAGE_ACCEPT, QR_IMAGE_FORMAT_NAMES } from './qr-formats';

import type { AddFromImage, AddFromImageStatus } from './add-from-image';
import type { Component } from 'solid-js';

export interface FromImageProps {
  adder: AddFromImage;
}

/**
 * Controlled and always empty: a chosen image goes straight to the scanner and
 * is not kept, so choosing the same file again (after a failed scan) is not
 * rejected as a duplicate.
 */
const NO_KEPT_FILES: File[] = [];

const STATUS_TEXT: Record<AddFromImageStatus, string> = {
  idle: '',
  picking: 'Waiting for the file dialog…',
  scanning: 'Reading the QR code…',
  adding: 'Adding the account…',
};

/**
 * The QR image tab: drop, choose or paste a screenshot of a QR code. "Choose
 * image…" opens the native file dialog (from Rust, with the image types to
 * pick from; Android's photo picker there), not the WebView's, so the drop
 * area itself does not open a picker.
 */
export const FromImage: Component<FromImageProps> = (props) => (
  <div class={'flex flex-col gap-3'}>
    {/* Grows with the panel: the drop area takes the room, and gives it back to an error. */}
    <FileUpload.Root
      class={'flex flex-1 flex-col'}
      accept={QR_IMAGE_ACCEPT}
      acceptedFiles={NO_KEPT_FILES}
      maxFiles={1}
      disabled={props.adder.status() !== 'idle'}
      onFileAccept={(details) => {
        if (details.files.length > 0) void props.adder.add(details.files[0]);
      }}
      onFileReject={() => props.adder.reject()}
    >
      <FileUpload.Dropzone
        disableClick
        class={
          'p-4 text-center border-2 border-border rounded-lg border-dashed bg-bg-input flex flex-1 flex-col gap-3 transition-colors items-center justify-center data-[dragging]:(border-primary bg-info-bg) data-[disabled]:opacity-60'
        }
      >
        <i class={'i-ph-qr-code text-text-muted size-8'} aria-hidden={'true'} />
        {/* A touch screen (Android) has nothing to drop or press Ctrl+V with. */}
        <p class={'text-sm text-text [@media(pointer:coarse)]:hidden'}>
          {'Drop a screenshot of the QR code here, or paste it with Ctrl+V.'}
        </p>
        <p class={'text-sm text-text hidden [@media(pointer:coarse)]:block'}>
          {'Choose a screenshot of the QR code.'}
        </p>
        <button
          type={'button'}
          class={'btn'}
          disabled={props.adder.status() !== 'idle'}
          onClick={() => void props.adder.choose()}
        >
          {'Choose image…'}
        </button>
        <p class={'subtle'}>{QR_IMAGE_FORMAT_NAMES}</p>
      </FileUpload.Dropzone>
    </FileUpload.Root>

    {/* Always present, so each status is announced as it changes. */}
    <p class={'subtle min-h-5'} aria-live={'polite'}>
      {STATUS_TEXT[props.adder.status()]}
    </p>

    <Show when={props.adder.error()}>
      {(error) => (
        <p class={'error'} role={'alert'}>
          {error()}
        </p>
      )}
    </Show>
  </div>
);
