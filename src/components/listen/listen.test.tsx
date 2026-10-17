import { render } from '@solidjs/testing-library';
import { describe, expect, it, vi } from 'vitest';

import { listenWhileMounted } from '.';

function mount(subscribe: () => Promise<() => void>, onError = vi.fn()) {
  return render(() => {
    listenWhileMounted(subscribe, onError);
    return null;
  });
}

describe('listenWhileMounted', () => {
  it('stops listening when the component unmounts', async () => {
    const stop = vi.fn();
    const { unmount } = mount(() => Promise.resolve(stop));
    await Promise.resolve();

    unmount();

    expect(stop).toHaveBeenCalledOnce();
  });

  it('stops a subscription that resolves after the component unmounted', async () => {
    const stop = vi.fn();
    let resolve!: (stop: () => void) => void;
    const { unmount } = mount(() => new Promise((done) => (resolve = done)));

    unmount();
    resolve(stop);
    await Promise.resolve();

    expect(stop).toHaveBeenCalledOnce();
  });

  it('reports a subscription that fails', async () => {
    const onError = vi.fn();
    mount(() => Promise.reject(new Error('no events')), onError);

    await vi.waitFor(() => expect(onError).toHaveBeenCalledOnce());
  });
});
