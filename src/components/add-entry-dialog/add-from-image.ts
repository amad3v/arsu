import { createSignal, onCleanup } from 'solid-js';

import { addEntryFromUri, pickQrImage } from '@api';
import { errorMessage } from '@api/lib';

import { QR_IMAGE_FORMAT_NAMES } from './qr-formats';
import { scanQrInWorker } from './scan-qr';

import type { QrScan } from './qr-image';
import type { Accessor } from 'solid-js';

/** Idle, waiting for the file dialog, scanning the image for a QR code, or adding the account it holds. */
export type AddFromImageStatus = 'idle' | 'picking' | 'scanning' | 'adding';

export interface AddFromImage {
  status: Accessor<AddFromImageStatus>;
  error: Accessor<string | null>;
  /** Scans `image` and adds the account its QR code holds. Ignored while busy. */
  add: (image: Blob) => Promise<void>;
  /** Asks for an image in the native file dialog, then adds it. Ignored while busy. */
  choose: () => Promise<void>;
  /** Reports a file that could not be used, e.g. one of the wrong type. */
  reject: () => void;
}

const SCAN_FAILURE: Record<Exclude<QrScan['status'], 'found'>, string> = {
  'not-found':
    'No QR code found in that image. Try a sharper screenshot that shows the whole code, or add the account by link or by hand.',
  unreadable: `That image can't be opened. Try a ${QR_IMAGE_FORMAT_NAMES} screenshot.`,
};

/**
 * Adding an account from a QR code image, shared by the QR image tab and the
 * dialog's paste handler. A scan still running when the owner is disposed
 * (the dialog closed) is cancelled; an add already sent to the backend is
 * not, and still reports its entry through `onAdded`.
 */
export function createAddFromImage(onAdded: (id: string) => void): AddFromImage {
  const [status, setStatus] = createSignal<AddFromImageStatus>('idle');
  const [error, setError] = createSignal<string | null>(null);
  const cancel = new AbortController();
  onCleanup(() => {
    cancel.abort();
  });

  async function choose() {
    if (status() !== 'idle') return;
    setError(null);
    setStatus('picking');

    let image: Blob | null;
    try {
      image = await pickQrImage();
    } catch (err) {
      setError(errorMessage(err));
      return;
    } finally {
      setStatus('idle');
    }
    if (image !== null) await add(image);
  }

  async function add(image: Blob) {
    if (status() !== 'idle') return;
    setError(null);
    setStatus('scanning');

    try {
      const scan = await scanQrInWorker(image, cancel.signal);
      if (scan.status !== 'found') {
        setError(SCAN_FAILURE[scan.status]);
        return;
      }
      setStatus('adding');
      onAdded(await addEntryFromUri(scan.text));
    } catch (err) {
      // Cancelled because the dialog closed: there is no one to tell.
      if (cancel.signal.aborted) return;
      setError(errorMessage(err));
    } finally {
      setStatus('idle');
    }
  }

  return {
    status,
    error,
    add,
    choose,
    reject: () => setError(`Choose one ${QR_IMAGE_FORMAT_NAMES} image.`),
  };
}
