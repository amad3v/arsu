import { render, screen, within } from '@solidjs/testing-library';
import { afterEach, describe, expect, it } from 'vitest';

import { toastIcon } from './icons';

import { GlobalToaster, toaster } from '.';

afterEach(() => {
  toaster.remove();
});

describe('GlobalToaster', () => {
  it('shows a toast with its title, description and a named dismiss button', async () => {
    render(() => <GlobalToaster />);

    toaster.create({ type: 'error', title: 'Import failed', description: 'The file is damaged.' });

    const toast = await screen.findByRole('status');
    expect(toast.dataset.type).toBe('error');
    expect(within(toast).getByText('Import failed')).not.toBeNull();
    expect(within(toast).getByText('The file is damaged.')).not.toBeNull();
    expect(within(toast).getByRole('button', { name: 'Dismiss notification' })).not.toBeNull();
  });

  it('renders no description element when there is none', async () => {
    render(() => <GlobalToaster />);

    toaster.create({ type: 'success', title: 'Theme saved' });

    const toast = await screen.findByRole('status');
    expect(toast.querySelector('[data-part="description"]')).toBeNull();
  });

  it('stacks toasts instead of overlapping them', async () => {
    render(() => <GlobalToaster />);

    toaster.create({ type: 'success', title: 'Imported 12 entries' });
    toaster.create({ type: 'info', title: 'Later news' });

    const toasts = await screen.findAllByRole('status');
    expect(toasts).toHaveLength(2);
    for (const toast of toasts) {
      expect(toast.hasAttribute('data-stack')).toBe(true);
      expect(toast.hasAttribute('data-overlap')).toBe(false);
    }
  });
});

describe('toastIcon', () => {
  it('gives each type its own icon shape', () => {
    const icons = ['success', 'error', 'warning', 'info'].map(
      (type) => toastIcon(type).split(' ')[0],
    );

    expect(new Set(icons).size).toBe(icons.length);
  });

  it('falls back to the info icon', () => {
    expect(toastIcon(undefined)).toBe(toastIcon('info'));
    expect(toastIcon('loading')).toBe(toastIcon('info'));
  });
});
