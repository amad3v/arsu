import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library';
import { createSignal, Show } from 'solid-js';
import { describe, expect, it } from 'vitest';

import { PasswordField } from '.';

import type { PasswordFieldProps } from '@app-types/ui';

function renderField(props: Partial<PasswordFieldProps> = {}) {
  const [value, setValue] = createSignal('');
  render(() => (
    <PasswordField
      label={'Master password'}
      value={value()}
      onValueChange={setValue}
      autocomplete={'current-password'}
      {...props}
    />
  ));
  return { input: screen.getByLabelText<HTMLInputElement>('Master password'), value };
}

describe('PasswordField', () => {
  it('labels a masked input and reports what is typed', () => {
    const { input, value } = renderField();

    expect(input.type).toBe('password');
    expect(input.autocomplete).toBe('current-password');

    fireEvent.input(input, { target: { value: 'hunter2' } });
    expect(value()).toBe('hunter2');
  });

  it('shows and hides the password from one source of truth', async () => {
    const { input } = renderField();

    // zag toggles on a primary-button pointerdown.
    fireEvent.pointerDown(screen.getByRole('button', { name: 'Show password' }));
    await waitFor(() => {
      expect(input.type).toBe('text');
    });

    fireEvent.pointerDown(screen.getByRole('button', { name: 'Hide password' }));
    await waitFor(() => {
      expect(input.type).toBe('password');
    });
  });

  it('marks the input invalid and describes it with the error', async () => {
    const { input } = renderField({ error: 'Incorrect password.' });

    expect(input.getAttribute('aria-invalid')).toBe('true');
    const errorText = screen.getByText('Incorrect password.');
    await waitFor(() => {
      expect(input.getAttribute('aria-describedby')?.split(' ')).toContain(errorText.id);
    });
  });

  it('shows no error when there is none', () => {
    const { input } = renderField({ error: null });

    expect(input.hasAttribute('aria-invalid')).toBe(false);
  });

  it('describes the input with its helper text', async () => {
    const { input } = renderField({ description: 'Use at least 12 characters.' });

    const helper = screen.getByText('Use at least 12 characters.');
    await waitFor(() => {
      expect(input.getAttribute('aria-describedby')?.split(' ')).toContain(helper.id);
    });
  });

  it('hands its input to the caller through ref', () => {
    let element: HTMLInputElement | undefined;
    const { input } = renderField({
      ref: (el) => {
        element = el;
      },
    });

    expect(element).toBe(input);
  });

  it('never focuses itself: a Modal gives it the initial focus through ref, a screen focuses it directly', () => {
    const [mounted, setMounted] = createSignal(true);
    render(() => (
      <Show when={mounted()}>
        <PasswordField
          label={'Master password'}
          value={''}
          onValueChange={() => undefined}
          autocomplete={'current-password'}
        />
      </Show>
    ));
    // Focusing itself on mount raced Ark's own focus-trap activation and
    // dropped keyboard focus to <body> on close.
    expect(document.activeElement).toBe(document.body);

    setMounted(false);
    setMounted(true);
    expect(document.activeElement).toBe(document.body);
  });

  describe('Caps Lock hint', () => {
    it('appears while Caps Lock is on and clears on blur', () => {
      const { input } = renderField({ capsLockHint: true });

      fireEvent.keyDown(input, { key: 'A', modifierCapsLock: true });
      expect(screen.queryByText('Caps Lock is on.')).not.toBeNull();

      fireEvent.keyUp(input, { key: 'a', modifierCapsLock: false });
      expect(screen.queryByText('Caps Lock is on.')).toBeNull();

      fireEvent.keyDown(input, { key: 'A', modifierCapsLock: true });
      fireEvent.blur(input);
      expect(screen.queryByText('Caps Lock is on.')).toBeNull();
    });

    it('stays off unless asked for', () => {
      const { input } = renderField();

      fireEvent.keyDown(input, { key: 'A', modifierCapsLock: true });
      expect(screen.queryByText('Caps Lock is on.')).toBeNull();
    });
  });
});
