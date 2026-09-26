import { cleanup } from '@solidjs/testing-library';
import { clearMocks } from '@tauri-apps/api/mocks';
import { afterEach, vi } from 'vitest';

// Vitest runs without globals, so @solidjs/testing-library cannot register its
// automatic cleanup: unmount rendered trees here. `clearMocks` drops any
// `mockIPC`/`mockWindows` handlers so IPC mocks never leak between tests.
afterEach(() => {
  cleanup();
  clearMocks();
});

// jsdom has no ResizeObserver, which floating-ui (tooltips, menus) and
// scrollFade use. Nothing is ever resized in jsdom, so observing is a no-op.
class ResizeObserverStub {
  observe = vi.fn();
  unobserve = vi.fn();
  disconnect = vi.fn();
}
// Defined directly, not with vi.stubGlobal: tests that unstub their own globals
// would remove it.
globalThis.ResizeObserver = ResizeObserverStub;
