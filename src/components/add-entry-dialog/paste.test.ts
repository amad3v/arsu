import { describe, expect, it } from 'vitest';

import { pasteAction } from './paste';

const png = new File(['png'], 'shot.png', { type: 'image/png' });
const svg = new File(['<svg/>'], 'code.svg', { type: 'image/svg+xml' });
const pdf = new File(['%PDF'], 'doc.pdf', { type: 'application/pdf' });
const link = 'otpauth://totp/GitHub:alice?secret=JBSWY3DPEHPK3PXP';

describe('pasteAction', () => {
  it('scans a pasted screenshot, even over a text field', () => {
    expect(pasteAction({ files: [png], text: '' }, false)).toEqual({ kind: 'image', image: png });
    expect(pasteAction({ files: [png], text: '' }, true)).toEqual({ kind: 'image', image: png });
  });

  it('picks the image among other pasted files', () => {
    expect(pasteAction({ files: [pdf, png], text: '' }, false)).toEqual({
      kind: 'image',
      image: png,
    });
  });

  it('ignores files it cannot scan', () => {
    expect(pasteAction({ files: [svg, pdf], text: '' }, false)).toBeNull();
  });

  it('sends a link pasted outside a text field to the link field', () => {
    expect(pasteAction({ files: [], text: `  ${link}\n` }, false)).toEqual({ kind: 'link', link });
    expect(pasteAction({ files: [], text: 'OTPAUTH://hotp/x?secret=A&counter=0' }, false)).toEqual({
      kind: 'link',
      link: 'OTPAUTH://hotp/x?secret=A&counter=0',
    });
  });

  it('lets text pasted into a text field land there', () => {
    expect(pasteAction({ files: [], text: link }, true)).toBeNull();
  });

  it('ignores any other text', () => {
    expect(pasteAction({ files: [], text: 'https://example.com' }, false)).toBeNull();
    expect(pasteAction({ files: [], text: '' }, false)).toBeNull();
  });
});
