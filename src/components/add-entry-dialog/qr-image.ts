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

/**
 * The size to decode an image at, within `budget` pixels, or null if the
 * image has no area.
 */
export function decodeSize(
  width: number,
  height: number,
  budget = MAX_DECODE_PIXELS,
): PixelSize | null {
  const area = width * height;
  if (!(width > 0 && height > 0 && Number.isFinite(area))) return null;

  // Scaling both sides keeps the shape. A side cannot shrink below one
  // pixel, so the other one is then capped to stay within the budget.
  const scale = Math.min(1, Math.sqrt(budget / area));
  const scaledWidth = Math.max(1, Math.floor(width * scale));
  const scaledHeight = Math.max(
    1,
    Math.min(Math.floor(height * scale), Math.floor(budget / scaledWidth)),
  );
  return {
    width: Math.min(scaledWidth, Math.floor(budget / scaledHeight)),
    height: scaledHeight,
  };
}

/**
 * The sizes to try decoding an image at, largest first: the full budget,
 * then a quarter and a sixteenth of it. jsQR binarises in small blocks, so
 * the code in a camera photo, blurred and taking only part of the picture,
 * often reads only once scaled down; a screenshot reads at the first size.
 */
export function decodeSizes(width: number, height: number): PixelSize[] {
  const sizes: PixelSize[] = [];
  let previous: PixelSize | null = null;
  for (const budget of [MAX_DECODE_PIXELS, MAX_DECODE_PIXELS / 4, MAX_DECODE_PIXELS / 16]) {
    const size = decodeSize(width, height, budget);
    if (size !== null && (previous === null || size.width < previous.width)) {
      sizes.push(size);
      previous = size;
    }
  }
  return sizes;
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

/** The Shape Detection API's barcode detector, where the platform has one. */
interface BarcodeDetectorClass {
  new (options: { formats: string[] }): {
    detect: (image: ImageBitmap) => Promise<{ rawValue: string }[]>;
  };
  getSupportedFormats: () => Promise<string[]>;
}

/**
 * The QR code's text as the platform's barcode detector reads it (on
 * Android, Google Play services' scanner, far better at photos than jsQR),
 * or null where there is none, it reads none, or it fails.
 */
async function detectWithPlatform(bitmap: ImageBitmap): Promise<string | null> {
  const detector = (globalThis as { BarcodeDetector?: BarcodeDetectorClass }).BarcodeDetector;
  if (detector === undefined) return null;
  try {
    if (!(await detector.getSupportedFormats()).includes('qr_code')) return null;
    const codes = await new detector({ formats: ['qr_code'] }).detect(bitmap);
    return codes.length > 0 ? codes[0].rawValue : null;
  } catch {
    return null;
  }
}

/**
 * Decodes the image, or takes the camera frame, and reads its QR code; the
 * bitmap is closed afterwards. Never rejects: the image comes from the user,
 * and any failure to decode it means it is unreadable.
 */
export async function scanQrImage(image: Blob | ImageBitmap): Promise<QrScan> {
  let bitmap: ImageBitmap;
  try {
    bitmap = image instanceof Blob ? await createImageBitmap(image) : image;
  } catch {
    return { status: 'unreadable' };
  }

  try {
    const sizes = decodeSizes(bitmap.width, bitmap.height);
    if (sizes.length === 0) return { status: 'unreadable' };

    const detected = await detectWithPlatform(bitmap);
    if (detected !== null) return { status: 'found', text: detected };

    for (const size of sizes) {
      const context = new OffscreenCanvas(size.width, size.height).getContext('2d');
      if (context === null) return { status: 'unreadable' };
      context.drawImage(bitmap, 0, 0, size.width, size.height);
      const text = readQrCode(context.getImageData(0, 0, size.width, size.height).data, size);
      if (text !== null) return { status: 'found', text };
    }
    return { status: 'not-found' };
  } catch {
    return { status: 'unreadable' };
  } finally {
    bitmap.close();
  }
}
