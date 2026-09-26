import { fireEvent, render, screen } from '@solidjs/testing-library';
import { mockIPC } from '@tauri-apps/api/mocks';
import { createSignal } from 'solid-js';
import { describe, expect, it } from 'vitest';

import { vaultExists } from '@api';

import type { Component } from 'solid-js';

// Smoke tests for the test harness itself: if these fail, the problem is the
// Vitest/jsdom/Solid setup, not application code.
describe('frontend test harness', () => {
  it('renders Solid components into jsdom with working reactivity', () => {
    const Counter: Component = () => {
      const [count, setCount] = createSignal(0);
      return (
        <button type={'button'} onClick={() => setCount((n) => n + 1)}>
          {`clicked ${count()}`}
        </button>
      );
    };

    render(() => <Counter />);
    const button = screen.getByRole('button');
    fireEvent.click(button);

    expect(button.textContent).toBe('clicked 1');
  });

  it('resolves tsconfig path aliases and routes invoke() through mockIPC', async () => {
    mockIPC((cmd) => cmd === 'vault_exists');

    await expect(vaultExists()).resolves.toBe(true);
  });
});
