import { describe, expect, it } from 'vitest';

import { countLabel } from '.';

describe('countLabel', () => {
  it('uses the singular for one and the plural otherwise', () => {
    expect(countLabel(0, 'entry', 'entries')).toBe('0 entries');
    expect(countLabel(1, 'entry', 'entries')).toBe('1 entry');
    expect(countLabel(2, 'entry', 'entries')).toBe('2 entries');
  });
});
