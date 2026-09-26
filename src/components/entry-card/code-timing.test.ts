import { describe, expect, it } from 'vitest';

import {
  elapsedInStep,
  expiryTime,
  isDue,
  isUrgent,
  liveCode,
  RETRY_AFTER_MS,
  secondsLeft,
  upcomingCode,
  URGENT_SECONDS,
} from './code-timing';

import type { CodeState } from './code-timing';

function state(overrides: Partial<CodeState> = {}): CodeState {
  return {
    code: null,
    nextCode: null,
    expiresAt: null,
    refreshAt: null,
    error: null,
    busy: false,
    ...overrides,
  };
}

describe('expiryTime', () => {
  it('ends the code on the whole second the backend counted from', () => {
    // Sent at 100.7 s; the backend saw second 100 and said 20 s were left.
    expect(expiryTime(100_700, 20)).toBe(120_000);
  });

  it('can only be early, never late, when the request straddles a second', () => {
    // Sent at 100.99 s, answered from second 101 with 19 s left: the real
    // expiry is 120 s. The estimate (119 s) is early, which only refetches.
    expect(expiryTime(100_990, 19)).toBeLessThanOrEqual(120_000);
  });

  it('follows any period the backend reports, not a fixed 30 s', () => {
    expect(expiryTime(0, 15)).toBe(15_000);
    expect(expiryTime(0, 60)).toBe(60_000);
    expect(expiryTime(0, 300)).toBe(300_000);
  });
});

describe('secondsLeft', () => {
  it('rounds up to whole seconds and never goes below zero', () => {
    expect(secondsLeft(20_000, 0)).toBe(20);
    expect(secondsLeft(20_000, 100)).toBe(20);
    expect(secondsLeft(20_000, 19_001)).toBe(1);
    expect(secondsLeft(20_000, 20_000)).toBe(0);
    expect(secondsLeft(20_000, 25_000)).toBe(0);
  });
});

describe('isUrgent', () => {
  it(`is true for the last ${URGENT_SECONDS} seconds`, () => {
    expect(isUrgent(URGENT_SECONDS + 1)).toBe(false);
    expect(isUrgent(URGENT_SECONDS)).toBe(true);
    expect(isUrgent(0)).toBe(true);
  });
});

describe('elapsedInStep', () => {
  it('is how far into its period a code is, within the period', () => {
    expect(elapsedInStep(30_000, 30, 0)).toBe(0);
    expect(elapsedInStep(30_000, 30, 12_000)).toBe(12_000);
    expect(elapsedInStep(30_000, 30, 31_000)).toBe(30_000);
    expect(elapsedInStep(60_000, 30, 0)).toBe(0);
  });
});

describe('isDue', () => {
  it('is due before the first fetch', () => {
    expect(isDue(state(), 0)).toBe(true);
  });

  it('is due once the refresh time has come, not before', () => {
    expect(isDue(state({ refreshAt: 30_000 }), 29_999)).toBe(false);
    expect(isDue(state({ refreshAt: 30_000 }), 30_000)).toBe(true);
  });

  it('is never due while a fetch is in flight', () => {
    expect(isDue(state({ busy: true }), 0)).toBe(false);
  });

  it('retries a failed fetch after a pause', () => {
    const failedAt = 1_000;
    const failed = state({ refreshAt: failedAt + RETRY_AFTER_MS, error: 'boom' });
    expect(isDue(failed, failedAt + 1_000)).toBe(false);
    expect(isDue(failed, failedAt + RETRY_AFTER_MS)).toBe(true);
  });
});

describe('liveCode', () => {
  it('is the code until it expires, then nothing if there is no next code yet', () => {
    const totp = state({ code: '123456', expiresAt: 30_000 });
    expect(liveCode(totp, 29_999)).toBe('123456');
    expect(liveCode(totp, 30_000)).toBeNull();
  });

  it('promotes the next code as soon as expiresAt passes, ahead of the refetch', () => {
    const totp = state({ code: '123456', nextCode: '654321', expiresAt: 30_000 });
    expect(liveCode(totp, 29_999)).toBe('123456');
    expect(liveCode(totp, 30_000)).toBe('654321');
  });

  it('keeps an HOTP code, which has no expiry', () => {
    expect(liveCode(state({ code: '123456' }), Number.MAX_SAFE_INTEGER)).toBe('123456');
  });

  it('is nothing before the first fetch', () => {
    expect(liveCode(state(), 0)).toBeNull();
  });
});

describe('upcomingCode', () => {
  const totp = state({ code: '123456', nextCode: '654321', expiresAt: 30_000 });

  it('previews the next code only in the last seconds', () => {
    expect(upcomingCode(totp, 30_000 - (URGENT_SECONDS + 1) * 1000)).toBeNull();
    expect(upcomingCode(totp, 30_000 - URGENT_SECONDS * 1000)).toBe('654321');
  });

  it('shows nothing once the code has expired', () => {
    expect(upcomingCode(totp, 30_000)).toBeNull();
  });

  it('shows nothing for HOTP', () => {
    expect(upcomingCode(state({ code: '123456', nextCode: '654321' }), 0)).toBeNull();
  });
});
