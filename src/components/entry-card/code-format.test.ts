import { describe, expect, it } from 'vitest';

import { codePlaceholder, groupDigits } from './code-format';

describe('groupDigits', () => {
  it('splits a code into two halves', () => {
    expect(groupDigits('123456')).toBe('123 456');
    expect(groupDigits('1234567')).toBe('123 4567');
    expect(groupDigits('12345678')).toBe('1234 5678');
  });

  it('keeps leading zeros', () => {
    expect(groupDigits('012345')).toBe('012 345');
  });
});

describe('codePlaceholder', () => {
  it('has the shape of the code it stands for', () => {
    expect(codePlaceholder(6)).toBe('••• •••');
    expect(codePlaceholder(8)).toBe('•••• ••••');
  });
});
