import { describe, expect, it } from 'vitest';

import { parseWholeNumber } from './whole-number';

describe('parseWholeNumber', () => {
  it.each([
    ['0', 0],
    ['30', 30],
    [' 42 ', 42],
    ['007', 7],
    [String(Number.MAX_SAFE_INTEGER), Number.MAX_SAFE_INTEGER],
  ])('reads %j as %d', (text, value) => {
    expect(parseWholeNumber(text)).toBe(value);
  });

  it.each(['', ' ', '-1', '+1', '1.5', '1e3', '30s', '0x10', '1 000', '9007199254740992'])(
    'rejects %j',
    (text) => {
      expect(parseWholeNumber(text)).toBeNull();
    },
  );
});
