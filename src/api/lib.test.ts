import { describe, expect, it } from 'vitest';

import { base64ToBytes, errorMessage, isAppError, isAppErrorPayload, svgToDataUri } from './lib';

const wrongPassword = { kind: 'WrongPassword', message: 'decryption failed' };

describe('isAppErrorPayload', () => {
  it('accepts the structured error every command rejects with', () => {
    expect(isAppErrorPayload(wrongPassword)).toBe(true);
  });

  it('accepts a kind this build does not know yet, so its message still shows', () => {
    expect(isAppErrorPayload({ kind: 'SomethingNew', message: 'text' })).toBe(true);
  });

  it.each([
    ['a Tauri-level string rejection', 'command copy_code not allowed'],
    ['null', null],
    ['undefined', undefined],
    ['an Error', new Error('boom')],
    ['an object without a message', { kind: 'Locked' }],
    ['an object without a kind', { message: 'vault is locked' }],
    ['a non-string kind', { kind: 3, message: 'x' }],
    ['a non-string message', { kind: 'Locked', message: 3 }],
  ])('rejects %s', (_name, err) => {
    expect(isAppErrorPayload(err)).toBe(false);
  });
});

describe('isAppError', () => {
  it('matches on kind alone, whatever the message says', () => {
    expect(
      isAppError({ kind: 'WrongPassword', message: 'reworded or translated' }, 'WrongPassword'),
    ).toBe(true);
  });

  it('does not match another kind', () => {
    expect(isAppError(wrongPassword, 'Locked')).toBe(false);
  });

  it('never matches on message text', () => {
    expect(isAppError({ kind: 'StorageIo', message: 'WrongPassword' }, 'WrongPassword')).toBe(
      false,
    );
    expect(isAppError('WrongPassword', 'WrongPassword')).toBe(false);
  });
});

describe('errorMessage', () => {
  it("shows an AppErrorPayload's message", () => {
    expect(errorMessage(wrongPassword)).toBe('decryption failed');
  });

  it('shows a plain-string rejection as-is', () => {
    expect(errorMessage('invalid args `entryId`')).toBe('invalid args `entryId`');
  });

  it("shows an Error's message", () => {
    expect(errorMessage(new Error('could not read the image'))).toBe('could not read the image');
  });

  it('stringifies anything else', () => {
    expect(errorMessage(42)).toBe('42');
    expect(errorMessage(null)).toBe('null');
  });
});

describe('svgToDataUri', () => {
  it('percent-encodes the document into a data: URI an <img> can load', () => {
    const svg = '<svg xmlns="http://www.w3.org/2000/svg"><rect width="1" height="1"/></svg>';
    const uri = svgToDataUri(svg);

    expect(uri.startsWith('data:image/svg+xml;charset=utf-8,')).toBe(true);
    expect(uri).not.toContain('<');
    expect(decodeURIComponent(uri.slice(uri.indexOf(',') + 1))).toBe(svg);
  });
});

describe('base64ToBytes', () => {
  it('decodes every byte value, not just text', () => {
    const bytes = Uint8Array.from({ length: 256 }, (_, index) => index);
    const base64 = btoa(String.fromCharCode(...bytes));
    expect(base64ToBytes(base64)).toEqual(bytes);
  });
});
