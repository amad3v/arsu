// The QR worker. Reading a QR code from a large screenshot takes jsQR long
// enough to freeze the window, so it runs here instead. It receives an image
// as a Blob, or a camera frame as an ImageBitmap (transferred), and replies
// to each with one QrScan, in order. Loaded by ./scan-qr.ts for one image
// and by the QR scanner's frame reader for a stream of frames, as a bundled
// module file, which the CSP's `script-src 'self'` allows (a blob: URL would
// not be).

import { scanQrImage } from './qr-image';

import type { QrScan } from './qr-image';

addEventListener('message', (event: MessageEvent) => {
  const image: unknown = event.data;
  const reply = (scan: QrScan) => {
    postMessage(scan);
  };

  if (image instanceof Blob || image instanceof ImageBitmap) {
    void scanQrImage(image).then(reply);
  } else {
    reply({ status: 'unreadable' });
  }
});
