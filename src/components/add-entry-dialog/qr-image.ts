// Reading a QR code out of an image. Runs in the QR worker (./qr-worker.ts),
// so jsQR stays out of the main bundle: the window imports only the types.
// The pure parts (sizing, jsQR) are unit-tested.

import jsQR from 'jsqr';

/**
 * Images with more pixels than this are scaled down before decoding. jsQR's
 * time grows with the pixel count, and a QR code large enough to scan from a
 * screen survives the scaling: a 4K screenshot keeps about 70% of its width.
 */
export const MAX_DECODE_PIXELS = 4_000_000;

export interface PixelSize {
  width: number;
  height: number;
}

/** The size to decode an image at, or null if the image has no area. */
export function decodeSize(width: number, height: number): PixelSize | null {
  const area = width * height;
  if (!(width > 0 && height > 0 && Number.isFinite(area))) return null;

  // Scaling both sides keeps the shape. A side cannot shrink below one
  // pixel, so the other one is then capped to stay within the budget.
  const scale = Math.min(1, Math.sqrt(MAX_DECODE_PIXELS / area));
  const scaledWidth = Math.max(1, Math.floor(width * scale));
  const scaledHeight = Math.max(
    1,
    Math.min(Math.floor(height * scale), Math.floor(MAX_DECODE_PIXELS / scaledWidth)),
  );
  return {
    width: Math.min(scaledWidth, Math.floor(MAX_DECODE_PIXELS / scaledHeight)),
    height: scaledHeight,
  };
}

/**
 * The text of the QR code in RGBA pixels, or null if there is none. Reads
 * dark-on-light codes and inverted ones, as in a dark-mode screenshot.
 */
export function readQrCode(pixels: Uint8ClampedArray, size: PixelSize): string | null {
  return jsQR(pixels, size.width, size.height, { inversionAttempts: 'attemptBoth' })?.data ?? null;
}

/** What scanning an image found. */
export type QrScan =
  | { status: 'found'; text: string }
  | { status: 'not-found' }
  /** Not an image this engine can decode, or one with no pixels. */
  | { status: 'unreadable' };

/**
 * Decodes the image and reads its QR code. Never rejects: the image comes
 * from the user, and any failure to decode it means it is unreadable.
 */
export async function scanQrImage(image: Blob): Promise<QrScan> {
  let bitmap: ImageBitmap;
  try {
    bitmap = await createImageBitmap(image);
  } catch {
    return { status: 'unreadable' };
  }

  try {
    const size = decodeSize(bitmap.width, bitmap.height);
    if (size === null) return { status: 'unreadable' };

    const context = new OffscreenCanvas(size.width, size.height).getContext('2d');
    if (context === null) return { status: 'unreadable' };

    context.drawImage(bitmap, 0, 0, size.width, size.height);
    const text = readQrCode(context.getImageData(0, 0, size.width, size.height).data, size);
    return text === null ? { status: 'not-found' } : { status: 'found', text };
  } catch {
    return { status: 'unreadable' };
  } finally {
    bitmap.close();
  }
}
