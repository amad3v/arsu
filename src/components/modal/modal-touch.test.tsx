import { render, screen } from '@solidjs/testing-library';
import { describe, expect, it, vi } from 'vitest';

import { Modal } from '.';

// The touch interface (Android).
vi.mock('@cpt/touch-ui', () => ({ isTouchUi: () => true }));

describe('Modal in the touch interface', () => {
  it('is a bottom sheet', async () => {
    render(() => (
      <Modal open title={'Delete GitHub?'} onClose={vi.fn()}>
        {'Body'}
      </Modal>
    ));

    const dialog = await screen.findByRole('dialog', { name: 'Delete GitHub?' });
    expect(dialog.classList.contains('sheet-content')).toBe(true);
  });

  it('is a full-screen page with `page`', async () => {
    render(() => (
      <Modal open page title={'About Arsu'} onClose={vi.fn()}>
        {'Body'}
      </Modal>
    ));

    const dialog = await screen.findByRole('dialog', { name: 'About Arsu' });
    expect(dialog.classList.contains('modal-page')).toBe(true);
  });
});
