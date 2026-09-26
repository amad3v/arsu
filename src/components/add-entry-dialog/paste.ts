import { isQrImageType } from './qr-formats';

/** What a paste into the add-entry dialog asks for. */
export type PasteAction = { kind: 'image'; image: File } | { kind: 'link'; link: string };

export interface PastedContent {
  files: readonly File[];
  text: string;
}

const OTPAUTH_LINK = /^otpauth:\/\//i;

/**
 * Routes a paste anywhere in the add-entry dialog: an image (a screenshot of
 * a QR code) is scanned, and an otpauth:// link goes to the link field. Text
 * pasted into a text field is left to land where it was pasted.
 */
export function pasteAction(content: PastedContent, intoTextField: boolean): PasteAction | null {
  const image = content.files.find((file) => isQrImageType(file.type));
  if (image !== undefined) return { kind: 'image', image };

  const link = content.text.trim();
  if (!intoTextField && OTPAUTH_LINK.test(link)) return { kind: 'link', link };

  return null;
}
