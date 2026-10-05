import { describe, expect, it, vi } from 'vitest';

import { registerOverlay } from '.';

/** What the Android activity does on Back. */
function pressBack(): boolean {
  return window.__arsuBack?.() ?? false;
}

describe('registerOverlay', () => {
  it('closes the overlay on top on Back, then the one under it, then lets Back leave', () => {
    const closeSettings = vi.fn();
    const closeSheet = vi.fn();
    const releaseSettings = registerOverlay(closeSettings);
    const releaseSheet = registerOverlay(closeSheet);

    expect(pressBack()).toBe(true);
    expect(closeSheet).toHaveBeenCalledOnce();
    expect(closeSettings).not.toHaveBeenCalled();
    releaseSheet();

    expect(pressBack()).toBe(true);
    expect(closeSettings).toHaveBeenCalledOnce();
    releaseSettings();

    expect(pressBack()).toBe(false);
  });

  it('forgets an overlay closed some other way', () => {
    const closeSettings = vi.fn();
    const closeSheet = vi.fn();
    const releaseSettings = registerOverlay(closeSettings);
    const releaseSheet = registerOverlay(closeSheet);

    // The sheet closes itself, by a choice made in it.
    releaseSheet();

    expect(pressBack()).toBe(true);
    expect(closeSheet).not.toHaveBeenCalled();
    expect(closeSettings).toHaveBeenCalledOnce();
    releaseSettings();
    expect(pressBack()).toBe(false);
  });

  it('leaves the history alone', () => {
    const start = history.length;
    const push = vi.spyOn(history, 'pushState');
    const release = registerOverlay(vi.fn());
    release();
    expect(push).not.toHaveBeenCalled();
    expect(history.length).toBe(start);
    push.mockRestore();
  });
});
