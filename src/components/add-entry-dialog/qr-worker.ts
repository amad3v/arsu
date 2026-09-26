// The QR worker. Reading a QR code from a large screenshot takes jsQR long
// enough to freeze the window, so it runs here instead. It receives the image
// as a Blob and replies with one QrScan. Loaded by ./scan-qr.ts as a bundled
// module file, which the CSP's `script-src 'self'` allows (a blob: URL would
// not be).

import { scanQrImage } from './qr-image';

import type { QrScan } from './qr-image';

addEventListener('message', (event: MessageEvent) => {
  const image: unknown = event.data;
  const reply = (scan: QrScan) => {
    postMessage(scan);
  };

  if (image instanceof Blob) {
    void scanQrImage(image).then(reply);
  } else {
    reply({ status: 'unreadable' });
  }
});
