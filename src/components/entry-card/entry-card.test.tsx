import { fireEvent, render, screen } from '@solidjs/testing-library';
import { createSignal } from 'solid-js';
import { createStore } from 'solid-js/store';
import { describe, expect, it, vi } from 'vitest';

import { EntryCard } from '.';

import type { EntryCardProps, EntryCode } from '.';
import type { CodeState } from './code-timing';
import type { EntrySummary } from '@app-types/api';
import type { SetStoreFunction } from 'solid-js/store';

const github: EntrySummary = {
  id: 'gh',
  issuer: 'GitHub',
  accountLabel: 'alice',
  otpType: 'totp',
  digits: 6,
  period: 30,
};

function entryCode(entry: EntrySummary, state: Partial<CodeState> = {}): EntryCode {
  const [store] = createStore<CodeState>({
    code: null,
    nextCode: null,
    expiresAt: null,
    refreshAt: null,
    error: null,
    busy: false,
    ...state,
  });
  return { entry, state: store, generate: vi.fn(() => Promise.resolve()) };
}

/** Like `entryCode`, but also returns the store's setter, for tests that need
 * to simulate a fetch resolving mid-test. */
function mutableEntryCode(
  entry: EntrySummary,
  state: Partial<CodeState> = {},
): { code: EntryCode; setState: SetStoreFunction<CodeState> } {
  const [store, setStore] = createStore<CodeState>({
    code: null,
    nextCode: null,
    expiresAt: null,
    refreshAt: null,
    error: null,
    busy: false,
    ...state,
  });
  return {
    code: { entry, state: store, generate: vi.fn(() => Promise.resolve()) },
    setState: setStore,
  };
}

function renderCard(props: Partial<EntryCardProps> & Pick<EntryCardProps, 'code'>, now = 0) {
  const [clock, setClock] = createSignal(now);
  const handlers = { onSelect: vi.fn(), onCopy: vi.fn(), onExport: vi.fn(), onDelete: vi.fn() };
  render(() => (
    <ul>
      <EntryCard
        now={clock}
        highlighted={false}
        copied={false}
        isNew={false}
        {...handlers}
        {...props}
      />
    </ul>
  ));
  return { ...handlers, setClock };
}

describe('EntryCard', () => {
  it('shows the code grouped, and copies it on click', () => {
    const code = entryCode(github, { code: '123456', nextCode: '654321', expiresAt: 30_000 });
    const { onCopy } = renderCard({ code });

    const button = screen.getByRole('button', {
      name: 'Copy the code for GitHub (alice): 123 456',
    });
    fireEvent.click(button);
    expect(onCopy).toHaveBeenCalledOnce();
    expect(screen.getByText('alice')).toBeTruthy();
  });

  it('counts down from the expiry, and warns with the next code in the last seconds', () => {
    const code = entryCode(github, { code: '123456', nextCode: '654321', expiresAt: 30_000 });
    const { setClock } = renderCard({ code }, 12_000);

    expect(screen.getByText('18 seconds left')).toBeTruthy();
    expect(screen.queryByText('654 321')).toBeNull();

    setClock(25_000);
    expect(screen.getByText('5 seconds left')).toBeTruthy();
    expect(screen.getByText('654 321')).toBeTruthy();
    expect(screen.getByText('5 seconds left').closest('[data-urgent]')).not.toBeNull();
  });

  it('never shows an expired code, and keeps the button focusable while none is due', () => {
    const code = entryCode(github, { code: '123456', expiresAt: 30_000 });
    renderCard({ code }, 30_000);

    const button = screen.getByRole('button', { name: 'No code yet for GitHub (alice)' });
    // aria-disabled, not the `disabled` attribute: the button stays in the tab
    // order instead of dropping keyboard focus to <body>.
    expect(button).toHaveProperty('disabled', false);
    expect(button.getAttribute('aria-disabled')).toBe('true');
    expect(screen.queryByText(/123/)).toBeNull();
  });

  it('promotes the next code as soon as it expires, without losing focus', () => {
    const code = entryCode(github, { code: '123456', nextCode: '654321', expiresAt: 30_000 });
    const { setClock } = renderCard({ code }, 12_000);

    const button = screen.getByRole('button', {
      name: 'Copy the code for GitHub (alice): 123 456',
    });
    button.focus();
    expect(document.activeElement).toBe(button);

    setClock(30_000);

    // Same DOM node: expired TOTP shows the promoted next code at once,
    // instead of blanking out while the refetch is in flight.
    expect(document.activeElement).toBe(button);
    expect(screen.getByRole('button', { name: 'Copy the code for GitHub (alice): 654 321' })).toBe(
      button,
    );
    expect(button.getAttribute('aria-disabled')).toBeNull();
  });

  it('asks before generating an HOTP code, on the same button that later copies it', () => {
    const { code, setState } = mutableEntryCode({ ...github, otpType: 'hotp', period: null });
    renderCard({ code });

    const button = screen.getByRole('button', { name: 'Generate code for GitHub (alice)' });
    button.focus();

    fireEvent.click(button);
    expect(code.generate).toHaveBeenCalledOnce();
    expect(document.activeElement).toBe(button);

    // The generate() call resolves and a code arrives; the row's one control
    // never gets replaced by a different element, so keyboard focus survives
    // the swap from "Generate" to the code.
    setState({ code: '111222' });
    expect(document.activeElement).toBe(button);
    expect(screen.getByRole('button', { name: 'Copy the code for GitHub (alice): 111 222' })).toBe(
      button,
    );
  });

  it('marks the highlighted row with more than colour', () => {
    renderCard({
      code: entryCode(github, { code: '123456', expiresAt: 30_000 }),
      highlighted: true,
    });

    expect(screen.getByRole('row').getAttribute('aria-current')).toBe('true');
    expect(screen.getByText('copies')).toBeTruthy();
  });

  it('confirms a copy in words', () => {
    renderCard({ code: entryCode(github, { code: '123456', expiresAt: 30_000 }), copied: true });
    expect(screen.getByText('Copied')).toBeTruthy();
  });

  it('shows a failed fetch in place of the account', () => {
    renderCard({ code: entryCode(github, { error: 'the clock is before 1970' }) });
    expect(screen.getByText('the clock is before 1970')).toBeTruthy();
    expect(screen.queryByText('alice')).toBeNull();
  });

  it('names its actions menu after the entry', () => {
    renderCard({ code: entryCode(github) });
    expect(screen.getByRole('button', { name: 'More actions for GitHub (alice)' })).toBeTruthy();
  });

  it('selects on a click and copies on a double-click, but leaves its buttons to themselves', () => {
    const code = entryCode(github, { code: '123456', expiresAt: 30_000 });
    const { onSelect, onCopy } = renderCard({ code });
    const title = screen.getByText('GitHub');

    // The mousedown's default is prevented, so the search keeps focus and no text is selected.
    expect(fireEvent.mouseDown(title)).toBe(false);
    expect(onSelect).toHaveBeenCalledOnce();
    fireEvent.dblClick(title);
    expect(onCopy).toHaveBeenCalledOnce();

    const actions = screen.getByRole('button', { name: /More actions for GitHub/ });
    expect(fireEvent.mouseDown(actions)).toBe(true);
    fireEvent.dblClick(actions);
    expect(onSelect).toHaveBeenCalledOnce();
    expect(onCopy).toHaveBeenCalledOnce();
  });
});
