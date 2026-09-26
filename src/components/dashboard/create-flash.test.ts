import { createRoot } from 'solid-js';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { createFlash } from './create-flash';

describe('createFlash', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('holds a value for the duration, then clears it', () => {
    createRoot((dispose) => {
      const [value, flash] = createFlash<string>(1500);
      expect(value()).toBeNull();

      flash('a');
      expect(value()).toBe('a');
      vi.advanceTimersByTime(1499);
      expect(value()).toBe('a');
      vi.advanceTimersByTime(1);
      expect(value()).toBeNull();
      dispose();
    });
  });

  it('restarts the time when set again', () => {
    createRoot((dispose) => {
      const [value, flash] = createFlash<string>(1500);

      flash('a');
      vi.advanceTimersByTime(1000);
      flash('b');
      vi.advanceTimersByTime(1000);
      expect(value()).toBe('b');
      vi.advanceTimersByTime(500);
      expect(value()).toBeNull();
      dispose();
    });
  });

  it('leaves no timer behind its owner', () => {
    createRoot((dispose) => {
      const [, flash] = createFlash<string>(1500);
      flash('a');
      dispose();
    });
    expect(vi.getTimerCount()).toBe(0);
  });
});
