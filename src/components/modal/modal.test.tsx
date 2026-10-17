import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library';
import { createSignal, untrack } from 'solid-js';
import { describe, expect, it, vi } from 'vitest';

import { PasswordField } from '@cpt/password-field';
import { dialogSettled } from '@cpt/testing/dialog';

import { Modal } from '.';

import type { ModalProps } from '@app-types/ui';

function renderModal(props: Partial<ModalProps> = {}) {
  const [open, setOpen] = createSignal(props.open ?? true);
  const onClose = vi.fn(() => setOpen(false));
  render(() => (
    <Modal title={'Delete entry'} {...props} open={open()} onClose={onClose}>
      <p>{'Body'}</p>
    </Modal>
  ));
  return { onClose, setOpen };
}

describe('Modal', () => {
  it('renders nothing while closed', () => {
    renderModal({ open: false });

    expect(screen.queryByRole('dialog')).toBeNull();
    expect(screen.queryByText('Body')).toBeNull();
  });

  it('is a dialog named by its title, with its content', () => {
    renderModal();

    const dialog = screen.getByRole('dialog', { name: 'Delete entry' });
    expect(dialog.textContent).toContain('Body');
  });

  it('announces the title once: no description unless one is given', async () => {
    renderModal();

    await waitFor(() => {
      expect(screen.getByRole('dialog').hasAttribute('aria-describedby')).toBe(false);
    });
  });

  it('is described by its description', async () => {
    renderModal({ description: 'This cannot be undone.' });

    await waitFor(() => {
      expect(screen.getByRole('dialog').getAttribute('aria-describedby')).toBe(
        screen.getByText('This cannot be undone.').id,
      );
    });
  });

  it('asks its owner to close from the named close button', async () => {
    const { onClose } = renderModal();

    fireEvent.click(screen.getByRole('button', { name: 'Close' }));

    await waitFor(() => {
      expect(screen.queryByRole('dialog')).toBeNull();
    });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('asks its owner to close on Escape', async () => {
    const { onClose } = renderModal();

    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' });

    await waitFor(() => {
      expect(onClose).toHaveBeenCalledTimes(1);
    });
  });

  it('asks initialFocusEl which element to focus, instead of a native [autofocus]', async () => {
    const [passwordInput, setPasswordInput] = createSignal<HTMLInputElement>();
    // jsdom has no layout, so the focus trap's own focusability check (which a
    // real browser passes) always rejects the candidate and falls back to the
    // dialog content; asserting on `initialFocusEl` itself is what's testable
    // here (see the same caveat on DeleteEntry's initial-focus test).
    // Ark calls this outside any reactive scope, so it reads the signal the
    // same way: untracked.
    const initialFocusEl = vi.fn(() => untrack(() => passwordInput() ?? null));
    render(() => (
      <Modal
        open
        title={'Export entry QR'}
        onClose={() => undefined}
        initialFocusEl={initialFocusEl}
      >
        <PasswordField
          ref={setPasswordInput}
          label={'Master password'}
          value={''}
          onValueChange={() => undefined}
          autocomplete={'current-password'}
        />
      </Modal>
    ));

    const input = await screen.findByLabelText('Master password');
    await waitFor(() => {
      expect(initialFocusEl).toHaveBeenCalled();
    });
    const results = initialFocusEl.mock.results;
    expect(results[results.length - 1]?.value).toBe(input);
  });

  it('returns focus to finalFocusEl once it closes', async () => {
    const trigger = document.createElement('button');
    document.body.appendChild(trigger);

    const { setOpen } = renderModal({ finalFocusEl: () => trigger });
    await screen.findByRole('dialog');
    await dialogSettled();

    setOpen(false);
    await waitFor(() => {
      expect(screen.queryByRole('dialog')).toBeNull();
    });
    await waitFor(() => {
      expect(document.activeElement).toBe(trigger);
    });

    trigger.remove();
  });

  it('opened from inside an open dialog, is not hidden from assistive technology', async () => {
    const [inner, setInner] = createSignal(false);
    render(() => (
      <Modal title={'Outer'} open onClose={vi.fn()}>
        <Modal title={'Inner'} open={inner()} onClose={() => setInner(false)}>
          <p>{'Inner body'}</p>
        </Modal>
      </Modal>
    ));
    // The outer dialog hides the rest of the page while the inner one is closed.
    await screen.findByRole('dialog', { name: 'Outer' });
    await dialogSettled();

    setInner(true);

    expect(await screen.findByRole('dialog', { name: 'Inner' })).toBeTruthy();
    await dialogSettled();
    expect(screen.getByRole('dialog', { name: 'Inner' })).toBeTruthy();
    // Closed again, it leaves nothing behind in the page.
    setInner(false);
    await waitFor(() => {
      expect(screen.queryByRole('dialog', { name: 'Inner', hidden: true })).toBeNull();
    });
  });

  it('follows its open prop', async () => {
    const { setOpen } = renderModal({ open: false });

    setOpen(true);
    expect(await screen.findByRole('dialog')).not.toBeNull();

    setOpen(false);
    await waitFor(() => {
      expect(screen.queryByRole('dialog')).toBeNull();
    });
  });
});
