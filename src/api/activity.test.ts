import { mockIPC } from '@tauri-apps/api/mocks';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { ACTIVITY_REPORT_INTERVAL_MS, reportUserActivity } from './activity';

/** Counts record_activity calls; `fail` makes each of them reject. */
function mockRecordActivity(fail = false): { count: () => number } {
  let calls = 0;
  mockIPC((cmd) => {
    if (cmd !== 'record_activity') throw new Error(`unexpected command ${cmd}`);
    calls += 1;
    if (fail) throw new Error('the backend task failed');
    return null;
  });
  return { count: () => calls };
}

/** Lets the mocked IPC promise, and anything chained on it, settle. */
async function settle(): Promise<void> {
  await vi.advanceTimersByTimeAsync(0);
}

describe('reportUserActivity', () => {
  let target: EventTarget;
  let stop: () => void;

  beforeEach(() => {
    vi.useFakeTimers();
    target = new EventTarget();
    stop = reportUserActivity(target);
  });

  afterEach(() => {
    stop();
    vi.useRealTimers();
  });

  it('reports the first input at once', () => {
    const backend = mockRecordActivity();

    target.dispatchEvent(new Event('keydown'));

    expect(backend.count()).toBe(1);
  });

  it(`reports at most once per ${ACTIVITY_REPORT_INTERVAL_MS} ms`, () => {
    const backend = mockRecordActivity();

    target.dispatchEvent(new Event('keydown'));
    vi.advanceTimersByTime(ACTIVITY_REPORT_INTERVAL_MS - 1);
    target.dispatchEvent(new Event('pointerdown'));
    expect(backend.count()).toBe(1);

    vi.advanceTimersByTime(1);
    target.dispatchEvent(new Event('wheel'));
    expect(backend.count()).toBe(2);
  });

  it.each(['keydown', 'pointerdown', 'wheel'])('counts %s as activity', (type) => {
    const backend = mockRecordActivity();

    target.dispatchEvent(new Event(type));

    expect(backend.count()).toBe(1);
  });

  it('ignores input that is not activity', () => {
    const backend = mockRecordActivity();

    target.dispatchEvent(new Event('mousemove'));
    target.dispatchEvent(new Event('focus'));

    expect(backend.count()).toBe(0);
  });

  it('reports again on the next input when a report fails', async () => {
    const backend = mockRecordActivity(true);

    target.dispatchEvent(new Event('keydown'));
    await settle();
    target.dispatchEvent(new Event('keydown'));

    expect(backend.count()).toBe(2);
  });

  it('stops reporting once stopped', () => {
    const backend = mockRecordActivity();

    stop();
    target.dispatchEvent(new Event('keydown'));

    expect(backend.count()).toBe(0);
  });
});
