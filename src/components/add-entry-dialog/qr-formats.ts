// The image formats a QR code is read from, for the file picker, paste
// routing and help text. Kept apart from ./qr-image.ts, which pulls in jsQR
// and belongs to the worker.

interface QrImageFormat {
  type: string;
  extensions: string[];
  name: string;
}

/**
 * The image formats a QR code is read from. Not SVG: a vector image may have
 * no pixel size, and there is nothing to decode it at.
 */
const QR_IMAGE_FORMATS: readonly QrImageFormat[] = [
  { type: 'image/png', extensions: ['.png'], name: 'PNG' },
  { type: 'image/jpeg', extensions: ['.jpg', '.jpeg'], name: 'JPEG' },
  { type: 'image/gif', extensions: ['.gif'], name: 'GIF' },
  { type: 'image/webp', extensions: ['.webp'], name: 'WebP' },
  { type: 'image/bmp', extensions: ['.bmp'], name: 'BMP' },
];

/** For a file picker's `accept`: each image type with its file extensions. */
export const QR_IMAGE_ACCEPT: Record<string, string[]> = Object.fromEntries(
  QR_IMAGE_FORMATS.map((format) => [format.type, format.extensions]),
);

const formatNames = QR_IMAGE_FORMATS.map((format) => format.name);

/** The formats for help text: "PNG, JPEG, GIF, WebP or BMP". */
export const QR_IMAGE_FORMAT_NAMES = `${formatNames.slice(0, -1).join(', ')} or ${formatNames[formatNames.length - 1]}`;

export function isQrImageType(type: string): boolean {
  return QR_IMAGE_FORMATS.some((format) => format.type === type);
}
