import { createRoot, createSignal } from 'solid-js';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { appError, deferred, mockCommands } from '@cpt/testing/ipc';

import { RETRY_AFTER_MS } from './code-timing';
import { createEntryCode } from './create-entry-code';

import type { EntryCode, EntryCodeOptions } from './create-entry-code';
import type { CodeResponse, EntrySummary } from '@app-types/api';
import type { Setter } from 'solid-js';

const totp: EntrySummary = {
  id: 'totp-id',
  issuer: 'GitHub',
  accountLabel: 'alice',
  otpType: 'totp',
  digits: 6,
  period: 45,
};

const hotp: EntrySummary = { ...totp, id: 'hotp-id', otpType: 'hotp', period: null };

/** Lets pending IPC promises settle; only Date is faked, so real timers still run. */
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

interface Mounted {
  code: EntryCode;
  setNow: Setter<number>;
  options: EntryCodeOptions;
  dispose: () => void;
}

function mount(entry: EntrySummary): Mounted {
  return createRoot((dispose) => {
    const [now, setNow] = createSignal(Date.now());
    const options = { now, onMissing: vi.fn(), onLocked: vi.fn() };
    return { code: createEntryCode(entry, options), setNow, options, dispose };
  });
}

describe('createEntryCode', () => {
  beforeEach(() => {
    vi.useFakeTimers({ toFake: ['Date'] });
    // 100.4 s after the epoch: 10.4 s into a 45 s step, which ends at 135 s.
    vi.setSystemTime(100_400);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('fetches a TOTP code at once, and again when the backend says it expires', async () => {
    const answers: CodeResponse[] = [
      { code: '111111', expiresInSeconds: 35, nextCode: '222222' },
      { code: '222222', expiresInSeconds: 45, nextCode: '333333' },
    ];
    const calls = mockCommands({ get_current_code: () => answers.shift() });
    const { code, setNow, dispose } = mount(totp);
    await settle();

    expect(calls).toEqual([{ cmd: 'get_current_code', args: { entryId: 'totp-id' } }]);
    expect(code.state.code).toBe('111111');
    expect(code.state.nextCode).toBe('222222');
    expect(code.state.expiresAt).toBe(135_000);

    // A 30 s step would roll over at 120 s; this entry's 45 s step does not.
    setNow(120_000);
    setNow(134_999);
    await settle();
    expect(calls).toHaveLength(1);

    setNow(135_000);
    await settle();
    expect(calls).toHaveLength(2);
    expect(code.state.code).toBe('222222');
    dispose();
  });

  it('never fetches an HOTP code on its own: each fetch uses a code up', async () => {
    const calls = mockCommands({
      get_current_code: () => ({ code: '424242', expiresInSeconds: null, nextCode: null }),
    });
    const { code, setNow, dispose } = mount(hotp);
    setNow(1_000_000);
    await settle();
    expect(calls).toHaveLength(0);

    await code.generate();
    expect(calls).toHaveLength(1);
    expect(code.state.code).toBe('424242');
    expect(code.state.expiresAt).toBeNull();

    setNow(9_000_000);
    await settle();
    expect(calls).toHaveLength(1);
    dispose();
  });

  it('asks for one HOTP code at a time, so a double press uses up one code', async () => {
    const answer = deferred<CodeResponse>();
    const calls = mockCommands({ get_current_code: () => answer.promise });
    const { code, dispose } = mount(hotp);

    const first = code.generate();
    await code.generate();
    answer.resolve({ code: '424242', expiresInSeconds: null, nextCode: null });
    await first;

    expect(calls).toHaveLength(1);
    expect(code.state.code).toBe('424242');
    dispose();
  });

  it('shows a failure, and tries again after a pause', async () => {
    const answers: (() => unknown)[] = [
      () => {
        throw appError('SystemClock', 'the clock is before 1970');
      },
      () => ({ code: '111111', expiresInSeconds: 35, nextCode: null }),
    ];
    const calls = mockCommands({ get_current_code: () => answers.shift()?.() });
    const { code, setNow, dispose } = mount(totp);
    await settle();
    expect(code.state.error).toBe('the clock is before 1970');

    setNow(100_400 + RETRY_AFTER_MS - 1);
    await settle();
    expect(calls).toHaveLength(1);

    setNow(100_400 + RETRY_AFTER_MS);
    await settle();
    expect(calls).toHaveLength(2);
    expect(code.state.error).toBeNull();
    expect(code.state.code).toBe('111111');
    dispose();
  });

  it('reports an entry deleted meanwhile, without showing an error', async () => {
    mockCommands({
      get_current_code: () => {
        throw appError('EntryNotFound');
      },
    });
    const { code, options, dispose } = mount(totp);
    await settle();

    expect(options.onMissing).toHaveBeenCalledOnce();
    expect(code.state.error).toBeNull();
    dispose();
  });

  it('reports a locked vault', async () => {
    mockCommands({
      get_current_code: () => {
        throw appError('Locked');
      },
    });
    const { options, dispose } = mount(totp);
    await settle();

    expect(options.onLocked).toHaveBeenCalledOnce();
    dispose();
  });
});
