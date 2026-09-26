import { fireEvent, render, screen } from '@solidjs/testing-library';
import { describe, expect, it, vi } from 'vitest';

import { BaseButton } from './base-btn';

import { IconifiedButton } from '.';

describe('IconifiedButton', () => {
  it('is named by its label, with a decorative icon', () => {
    render(() => <IconifiedButton icon={'i-ph-lock'} label={'Lock vault'} />);

    const button = screen.getByRole('button', { name: 'Lock vault' });
    const icon = button.querySelector('i');
    expect(icon?.getAttribute('aria-hidden')).toBe('true');
    expect(icon?.classList.contains('i-ph-lock')).toBe(true);
  });

  it("runs the caller's click handler alongside the tooltip's", () => {
    const onClick = vi.fn();
    render(() => <IconifiedButton icon={'i-ph-lock'} label={'Lock vault'} onClick={onClick} />);

    fireEvent.click(screen.getByRole('button', { name: 'Lock vault' }));

    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it('shows the label as its tooltip', async () => {
    render(() => <IconifiedButton icon={'i-ph-lock'} label={'Lock vault'} />);

    fireEvent.pointerMove(screen.getByRole('button', { name: 'Lock vault' }));

    expect((await screen.findByRole('tooltip')).textContent).toBe('Lock vault');
  });

  it('keeps no tooltip in the document until it is shown', () => {
    render(() => <IconifiedButton icon={'i-ph-lock'} label={'Lock vault'} />);

    expect(document.querySelector('[data-scope="tooltip"][data-part="content"]')).toBeNull();
  });
});

describe('BaseButton', () => {
  it('defaults to a non-submitting button with a size-5 icon', () => {
    render(() => <BaseButton icon={'i-ph-copy'} label={'Copy code'} />);

    const button = screen.getByRole<HTMLButtonElement>('button', { name: 'Copy code' });
    expect(button.type).toBe('button');
    expect(button.querySelector('i')?.classList.contains('size-5')).toBe(true);
  });

  it('lets the caller override its defaults', () => {
    render(() => (
      <BaseButton icon={'i-ph-copy'} label={'Copy code'} iconSz={'size-4'} type={'submit'} />
    ));

    const button = screen.getByRole<HTMLButtonElement>('button', { name: 'Copy code' });
    expect(button.type).toBe('submit');
    const icon = button.querySelector('i');
    expect(icon?.classList.contains('size-4')).toBe(true);
    expect(icon?.classList.contains('size-5')).toBe(false);
  });
});
