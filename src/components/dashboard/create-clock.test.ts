import { createRoot, createSignal } from 'solid-js';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { createClock, msUntilNextSecond } from './create-clock';

describe('msUntilNextSecond', () => {
  it('is the time to the next whole second', () => {
    expect(msUntilNextSecond(1_000_300)).toBe(700);
    expect(msUntilNextSecond(1_000_999)).toBe(1);
    expect(msUntilNextSecond(1_000_000)).toBe(1000);
  });
});

/** A clock under its own root; effects run once the root is created. */
function mount(initiallyRunning: boolean) {
  return createRoot((dispose) => {
    const [running, setRunning] = createSignal(initiallyRunning);
    return { now: createClock(running), setRunning, dispose };
  });
}

describe('createClock', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(1_000_300);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('ticks on whole seconds, when displayed seconds change', () => {
    const { now, dispose } = mount(true);
    expect(now()).toBe(1_000_300);

    vi.advanceTimersByTime(699);
    expect(now()).toBe(1_000_300);

    vi.advanceTimersByTime(1);
    expect(now()).toBe(1_001_000);

    vi.advanceTimersByTime(1000);
    expect(now()).toBe(1_002_000);
    dispose();
  });

  it('runs only while asked to', () => {
    const { now, setRunning, dispose } = mount(false);

    vi.advanceTimersByTime(5000);
    expect(now()).toBe(1_000_300);
    expect(vi.getTimerCount()).toBe(0);

    setRunning(true);
    expect(now()).toBe(1_005_300);
    expect(vi.getTimerCount()).toBe(1);
    vi.advanceTimersByTime(700);
    expect(now()).toBe(1_006_000);

    setRunning(false);
    expect(vi.getTimerCount()).toBe(0);
    dispose();
  });

  it('stops with its owner', () => {
    const { dispose } = mount(true);
    expect(vi.getTimerCount()).toBe(1);

    dispose();
    expect(vi.getTimerCount()).toBe(0);
  });
});
