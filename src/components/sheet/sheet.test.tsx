import { fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library';
import { createSignal } from 'solid-js';
import { describe, expect, it, vi } from 'vitest';

import { Sheet, SheetModal } from '.';

describe('SheetModal', () => {
  it('is a dialog named by its title and described by its description', async () => {
    render(() => (
      <SheetModal
        open
        title={'Delete GitHub?'}
        description={"This can't be undone."}
        onClose={vi.fn()}
      >
        <button type={'button'}>{'Cancel'}</button>
      </SheetModal>
    ));

    const sheet = await screen.findByRole('dialog', { name: 'Delete GitHub?' });
    expect(sheet.getAttribute('aria-describedby')).toBeTruthy();
    expect(within(sheet).getByText("This can't be undone.")).toBeTruthy();
    expect(within(sheet).getByRole('button', { name: 'Cancel' })).toBeTruthy();
  });
});

describe('Sheet', () => {
  it('reports its exit once it has closed', async () => {
    const onExitComplete = vi.fn();
    const [open, setOpen] = createSignal(true);
    render(() => (
      <Sheet
        open={open()}
        title={'Actions'}
        onClose={() => setOpen(false)}
        onExitComplete={onExitComplete}
      >
        <button type={'button'} onClick={() => setOpen(false)}>
          {'Delete'}
        </button>
      </Sheet>
    ));

    fireEvent.click(await screen.findByRole('button', { name: 'Delete' }));

    await waitFor(() => expect(onExitComplete).toHaveBeenCalledOnce());
    expect(screen.queryByRole('dialog', { name: 'Actions' })).toBeNull();
  });
});
