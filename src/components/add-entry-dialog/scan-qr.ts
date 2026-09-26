import type { QrScan } from './qr-image';

/**
 * Reads the QR code in `image` in a worker, so the window stays responsive.
 * Each scan has its own worker, terminated as soon as the scan settles or
 * `signal` aborts.
 *
 * Rejects with an `AbortError` DOMException if `signal` aborts first, and
 * with an Error if the worker cannot run.
 */
export function scanQrInWorker(image: Blob, signal: AbortSignal): Promise<QrScan> {
  return new Promise<QrScan>((resolve, reject) => {
    const aborted = () => new DOMException('The QR code scan was cancelled.', 'AbortError');
    if (signal.aborted) {
      reject(aborted());
      return;
    }

    const worker = new Worker(new URL('./qr-worker.ts', import.meta.url), { type: 'module' });
    const stop = () => {
      worker.terminate();
      signal.removeEventListener('abort', onAbort);
    };
    const onAbort = () => {
      stop();
      reject(aborted());
    };

    signal.addEventListener('abort', onAbort);
    // The worker replies exactly once, with a QrScan.
    worker.addEventListener('message', (event: MessageEvent<QrScan>) => {
      stop();
      resolve(event.data);
    });
    worker.addEventListener('error', () => {
      stop();
      reject(new Error('The QR code reader could not start.'));
    });
    worker.postMessage(image);
  });
}
