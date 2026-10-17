import { describe, expect, it } from 'vitest';

import { describeSummary, isPending } from './summary';

import type { SyncSummary } from '@app-types/api';

const nothing: SyncSummary = {
  peerName: 'Computer',
  added: 0,
  updated: 0,
  deleted: 0,
  paired: false,
};

describe('describeSummary', () => {
  it('says when a sync changed nothing', () => {
    expect(describeSummary(nothing)).toBe('Synced with Computer. Nothing changed here.');
  });

  it('counts what changed, in words', () => {
    expect(describeSummary({ ...nothing, added: 2, updated: 1, deleted: 1 })).toBe(
      'Synced with Computer: 2 accounts added, 1 updated, 1 deleted.',
    );
    expect(describeSummary({ ...nothing, added: 1 })).toBe(
      'Synced with Computer: 1 account added.',
    );
  });

  it('leads with the pairing when there was one', () => {
    expect(describeSummary({ ...nothing, paired: true, added: 3 })).toBe(
      'Paired with Computer: 3 accounts added.',
    );
    expect(describeSummary({ ...nothing, paired: true })).toBe(
      'Paired with Computer. Nothing changed here.',
    );
  });
});

describe('isPending', () => {
  it('is a device dated after any real time: waiting for its reply', () => {
    // u64::MAX from Rust, the way a JSON number that large reaches JavaScript.
    expect(isPending(JSON.parse('18446744073709551615') as number)).toBe(true);
    expect(isPending(1_760_000_000)).toBe(false);
  });
});
