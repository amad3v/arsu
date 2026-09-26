import { describe, expect, it } from 'vitest';

import { appError } from '@cpt/testing/ipc';

import {
  codeSettingsSummary,
  EMPTY_MANUAL_ENTRY,
  fieldForError,
  secretWarning,
  validateManualEntry,
} from './manual-entry';

import type { ManualEntryDraft } from './manual-entry';

function draft(changes: Partial<ManualEntryDraft>): ManualEntryDraft {
  return { ...EMPTY_MANUAL_ENTRY, accountLabel: 'alice', secret: 'JBSWY3DPEHPK3PXP', ...changes };
}

function errorsOf(changes: Partial<ManualEntryDraft>) {
  const result = validateManualEntry(draft(changes));
  return result.valid ? {} : result.errors;
}

describe('validateManualEntry', () => {
  it('builds a TOTP entry with the usual defaults', () => {
    expect(validateManualEntry(draft({}))).toEqual({
      valid: true,
      input: {
        issuer: null,
        accountLabel: 'alice',
        secretBase32: 'JBSWY3DPEHPK3PXP',
        algorithm: 'sha1',
        digits: 6,
        type: 'totp',
        period: 30,
      },
    });
  });

  it('builds an HOTP entry with its counter and no period', () => {
    expect(
      validateManualEntry(
        draft({ type: 'hotp', counter: '12', algorithm: 'sha256', digits: '8', period: 'x' }),
      ),
    ).toEqual({
      valid: true,
      input: {
        issuer: null,
        accountLabel: 'alice',
        secretBase32: 'JBSWY3DPEHPK3PXP',
        algorithm: 'sha256',
        digits: 8,
        type: 'hotp',
        counter: 12,
      },
    });
  });

  it('trims the names and passes the secret as typed', () => {
    const result = validateManualEntry(
      draft({ issuer: '  GitHub ', accountLabel: ' alice ', secret: 'jbsw y3dp ehpk 3pxp==' }),
    );

    expect(result).toMatchObject({
      valid: true,
      input: { issuer: 'GitHub', accountLabel: 'alice', secretBase32: 'jbsw y3dp ehpk 3pxp==' },
    });
  });

  describe('names', () => {
    it('accepts an issuer alone, which the backend makes the account name', () => {
      expect(validateManualEntry(draft({ issuer: 'GitHub', accountLabel: '  ' }))).toMatchObject({
        valid: true,
        input: { issuer: 'GitHub', accountLabel: '' },
      });
    });

    it('needs an account name or an issuer', () => {
      expect(errorsOf({ issuer: ' ', accountLabel: '' })).toEqual({
        accountLabel: 'Enter an account name, or at least an issuer.',
      });
    });

    it('rejects a colon in either name', () => {
      const errors = errorsOf({ issuer: 'Git:Hub', accountLabel: 'al:ice' });

      expect(errors.issuer).toContain('colon');
      expect(errors.accountLabel).toContain('colon');
    });
  });

  describe('secret', () => {
    it.each(['', '   ', '===='])('needs one: %j', (secret) => {
      expect(errorsOf({ secret })).toEqual({ secret: 'Enter the secret key.' });
    });

    it.each(['JBSWY3DPEHPK3PX1', 'JBSW-Y3DP', 'JBSWY3DPEHPK3PX8', 'JBSW=Y3DP', 'ÄBCD'])(
      'accepts only base32 letters and digits: %j',
      (secret) => {
        expect(errorsOf({ secret })).toEqual({
          secret: 'A secret key has only the letters A–Z and the digits 2–7.',
        });
      },
    );
  });

  describe('period', () => {
    it.each(['1', '30', '300', ' 60 '])('accepts %j', (period) => {
      expect(validateManualEntry(draft({ period })).valid).toBe(true);
    });

    it.each(['', '0', '301', '1.5', '-30', '30s', '1e2'])(
      'rejects %j instead of replacing it with a default',
      (period) => {
        expect(errorsOf({ period })).toEqual({
          period: 'Use a whole number of seconds from 1 to 300.',
        });
      },
    );
  });

  describe('counter', () => {
    it.each(['0', '1', String(Number.MAX_SAFE_INTEGER)])('accepts %j', (counter) => {
      expect(validateManualEntry(draft({ type: 'hotp', counter })).valid).toBe(true);
    });

    it.each(['', '-1', '1.5', '9007199254740993'])(
      'rejects %j instead of replacing it with a default',
      (counter) => {
        expect(errorsOf({ type: 'hotp', counter })).toEqual({
          counter: 'Use a whole number, 0 or more.',
        });
      },
    );
  });

  it('reports every invalid field at once', () => {
    expect(Object.keys(errorsOf({ accountLabel: '', secret: '', period: '0' }))).toEqual([
      'accountLabel',
      'secret',
      'period',
    ]);
  });
});

describe('fieldForError', () => {
  it.each([
    ['MissingLabel', 'accountLabel'],
    ['LabelContainsColon', 'accountLabel'],
    ['InvalidSecret', 'secret'],
    ['EmptySecret', 'secret'],
    ['InvalidPeriod', 'period'],
    ['CounterExhausted', 'counter'],
  ] as const)('puts %s on the %s field', (kind, field) => {
    expect(fieldForError(appError(kind))).toBe(field);
  });

  it('leaves other errors to the form as a whole', () => {
    expect(fieldForError(appError('StorageIo'))).toBeNull();
    expect(fieldForError(new Error('network'))).toBeNull();
    expect(fieldForError('a Tauri rejection')).toBeNull();
  });
});

describe('codeSettingsSummary', () => {
  it('sums up the code settings in one line', () => {
    expect(codeSettingsSummary(EMPTY_MANUAL_ENTRY)).toBe('TOTP · SHA-1 · 6 digits · 30 s');
    expect(
      codeSettingsSummary({
        ...EMPTY_MANUAL_ENTRY,
        type: 'hotp',
        algorithm: 'sha512',
        digits: '8',
      }),
    ).toBe('HOTP · SHA-512 · 8 digits · counter 0');
  });
});

describe('secretWarning', () => {
  it('warns about a key shorter than services issue (80 bits, 16 characters)', () => {
    // "whatever" is valid base32: 8 characters, 40 bits.
    expect(secretWarning('whatever')).toMatch(/shorter than services' keys \(at least 16/);
    expect(secretWarning('JBSW Y3DP EHPK 3PX')).not.toBeNull();
  });

  it("accepts services' usual lengths, however they are spaced or padded", () => {
    expect(secretWarning('JBSWY3DPEHPK3PXP')).toBeNull(); // 80 bits
    expect(secretWarning('jbsw y3dp ehpk 3pxp')).toBeNull();
    expect(secretWarning('HXDMVJECJJWSRB3HWIZR4IFUGFTMXBOZ')).toBeNull(); // 160 bits
  });

  it('leaves an invalid or empty key to the error', () => {
    expect(secretWarning('')).toBeNull();
    expect(secretWarning('not base32!')).toBeNull();
  });
});
