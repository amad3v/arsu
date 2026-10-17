// Reads camera frames in the QR worker (see add-entry-dialog/qr-worker.ts),
// one at a time: the scanner sends the next frame once the last is read, so
// a slow phone reads fewer frames rather than falling behind.

import type { QrScan } from '@cpt/add-entry-dialog/qr-image';

export interface FrameReader {
  /** Reads the QR code in `frame`, which is handed over to the worker. */
  read: (frame: ImageBitmap) => Promise<QrScan>;
  /** Stops the worker; a read still running then never settles. */
  close: () => void;
}

/** A reader with a worker of its own, kept for as long as the scanner is open. */
export function createFrameReader(): FrameReader {
  const worker = new Worker(new URL('../add-entry-dialog/qr-worker.ts', import.meta.url), {
    type: 'module',
  });

  // One frame at a time, so one answer is ever awaited.
  let waiting: ((scan: QrScan) => void) | null = null;
  const answer = (scan: QrScan) => {
    const resolve = waiting;
    waiting = null;
    resolve?.(scan);
  };
  worker.addEventListener('message', (event: MessageEvent<QrScan>) => answer(event.data));
  // A worker that cannot run reads nothing; the scanner goes on trying.
  worker.addEventListener('error', () => answer({ status: 'unreadable' }));

  return {
    read: (frame) =>
      new Promise<QrScan>((resolve) => {
        waiting = resolve;
        worker.postMessage(frame, [frame]);
      }),
    close: () => worker.terminate(),
  };
}
