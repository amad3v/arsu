import { describe, expect, it } from 'vitest';

import { countLabel, fileSize } from '.';

describe('countLabel', () => {
  it('uses the singular for one and the plural otherwise', () => {
    expect(countLabel(0, 'entry', 'entries')).toBe('0 entries');
    expect(countLabel(1, 'entry', 'entries')).toBe('1 entry');
    expect(countLabel(2, 'entry', 'entries')).toBe('2 entries');
  });
});

describe('fileSize', () => {
  it('counts bytes, then kilobytes and megabytes to one decimal', () => {
    expect(fileSize(1)).toBe('1 byte');
    expect(fileSize(640)).toBe('640 bytes');
    expect(fileSize(12_345)).toBe('12.3 kB');
    expect(fileSize(1_234_567)).toBe('1.2 MB');
  });
});
