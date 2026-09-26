import { describe, expect, it } from 'vitest';

import { entryLabel } from '.';

describe('entryLabel', () => {
  it('names the service and the account', () => {
    expect(entryLabel({ issuer: 'GitHub', accountLabel: 'alice' })).toBe('GitHub (alice)');
  });

  it('is just the account name without an issuer', () => {
    expect(entryLabel({ issuer: null, accountLabel: 'alice@example.com' })).toBe(
      'alice@example.com',
    );
  });
});
