import { describe, expect, it } from 'vitest';

import {
  checkNewPassword,
  MIN_PASSWORD_CHARS,
  meetsPasswordPolicy,
  passwordLength,
} from './password-policy';

const twelve = 'abcdefghijkl';

describe('passwordLength', () => {
  it('counts code points, as the backend does, not UTF-16 units', () => {
    expect('🔑'.length).toBe(2);
    expect(passwordLength('🔑')).toBe(1);
    expect(passwordLength('pässwörd')).toBe(8);
  });
});

describe('meetsPasswordPolicy', () => {
  it(`requires ${MIN_PASSWORD_CHARS} characters`, () => {
    expect(meetsPasswordPolicy(twelve.slice(1))).toBe(false);
    expect(meetsPasswordPolicy(twelve)).toBe(true);
  });

  it('does not let astral characters count twice', () => {
    // 6 characters, 12 UTF-16 units: the backend rejects this.
    expect(meetsPasswordPolicy('🔑🔑🔑🔑🔑🔑')).toBe(false);
  });
});

describe('checkNewPassword', () => {
  it('states the rule while nothing is typed, without flagging the empty confirmation', () => {
    expect(checkNewPassword('', '')).toEqual({
      password: 'Use at least 12 characters.',
      confirmation: null,
      valid: false,
    });
  });

  it('says how many characters are still missing', () => {
    expect(checkNewPassword('abcde', '').password).toBe(
      'Use at least 12 characters: 7 more to go.',
    );
  });

  it('flags a confirmation that differs once one is typed', () => {
    expect(checkNewPassword(twelve, 'abc')).toEqual({
      password: null,
      confirmation: "The passwords don't match.",
      valid: false,
    });
  });

  it('is valid once the password is long enough and confirmed', () => {
    expect(checkNewPassword(twelve, twelve)).toEqual({
      password: null,
      confirmation: null,
      valid: true,
    });
  });

  it('is not valid for a matching pair that is too short', () => {
    expect(checkNewPassword('short', 'short').valid).toBe(false);
  });
});
