import { describe, expect, it } from 'vitest';

import { isQrImageType, QR_IMAGE_ACCEPT, QR_IMAGE_FORMAT_NAMES } from './qr-formats';

describe('accepted image formats', () => {
  it('are the raster formats, each with its extensions', () => {
    expect(QR_IMAGE_ACCEPT).toEqual({
      'image/png': ['.png'],
      'image/jpeg': ['.jpg', '.jpeg'],
      'image/gif': ['.gif'],
      'image/webp': ['.webp'],
      'image/bmp': ['.bmp'],
    });
    expect(QR_IMAGE_FORMAT_NAMES).toBe('PNG, JPEG, GIF, WebP or BMP');
  });

  it('leave out SVG, which may have no pixel size', () => {
    expect(isQrImageType('image/png')).toBe(true);
    expect(isQrImageType('image/svg+xml')).toBe(false);
    expect(isQrImageType('')).toBe(false);
  });
});
