import { createRoot } from 'solid-js';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { appError, deferred, mockCommands } from '@cpt/testing/ipc';
import { toaster } from '@cpt/toaster';

import { createEntryList } from './create-entry-list';

import type { EntryList } from './create-entry-list';
import type { EntrySummary } from '@app-types/api';

function entry(id: string, issuer: string | null = null): EntrySummary {
  return { id, issuer, accountLabel: `account-${id}`, otpType: 'totp', digits: 6, period: 30 };
}

/** Lets pending IPC promises settle. */
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

function mount(onLocked = vi.fn()): { list: EntryList; dispose: () => void } {
  return createRoot((dispose) => ({ list: createEntryList({ onLocked }), dispose }));
}

describe('createEntryList', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('loads the entries at once', async () => {
    mockCommands({ list_entries: () => [entry('a'), entry('b')] });
    const { list, dispose } = mount();
    expect(list.state.status).toBe('loading');

    await settle();
    expect(list.state.status).toBe('ready');
    expect(list.state.entries.map((e) => e.id)).toEqual(['a', 'b']);
    dispose();
  });

  it('keeps each entry object across refreshes, so rows keep their state', async () => {
    const answers = [
      [entry('a'), entry('b')],
      [entry('a', 'Renamed'), entry('b'), entry('c')],
    ];
    mockCommands({ list_entries: () => answers.shift() });
    const { list, dispose } = mount();
    await settle();
    const [a, b] = list.state.entries;

    await list.refresh();
    expect(list.state.entries.map((e) => e.id)).toEqual(['a', 'b', 'c']);
    expect(list.state.entries[0]).toBe(a);
    expect(list.state.entries[1]).toBe(b);
    expect(list.state.entries[0].issuer).toBe('Renamed');
    dispose();
  });

  it('ignores an answer that arrives after a newer one', async () => {
    const slow = deferred<EntrySummary[]>();
    const answers: (() => unknown)[] = [() => [], () => slow.promise, () => [entry('new')]];
    mockCommands({ list_entries: () => answers.shift()?.() });
    const { list, dispose } = mount();
    await settle();

    const older = list.refresh();
    await list.refresh();
    slow.resolve([entry('old')]);
    await older;

    expect(list.state.entries.map((e) => e.id)).toEqual(['new']);
    dispose();
  });

  it('reports a failed first load, and loads again on retry', async () => {
    const answers: (() => unknown)[] = [
      () => {
        throw appError('StorageIo', 'disk on fire');
      },
      () => [entry('a')],
    ];
    mockCommands({ list_entries: () => answers.shift()?.() });
    const { list, dispose } = mount();
    await settle();
    expect(list.state.status).toBe('failed');
    expect(list.state.error).toBe('disk on fire');

    const retry = list.refresh();
    expect(list.state.status).toBe('loading');
    await retry;
    expect(list.state.status).toBe('ready');
    expect(list.state.error).toBeNull();
    dispose();
  });

  it('keeps the list and says so when a later refresh fails', async () => {
    const toast = vi.spyOn(toaster, 'create');
    const answers: (() => unknown)[] = [
      () => [entry('a')],
      () => {
        throw appError('StorageIo', 'disk on fire');
      },
    ];
    mockCommands({ list_entries: () => answers.shift()?.() });
    const { list, dispose } = mount();
    await settle();

    await list.refresh();
    expect(list.state.status).toBe('ready');
    expect(list.state.entries.map((e) => e.id)).toEqual(['a']);
    expect(toast).toHaveBeenCalledWith(
      expect.objectContaining({ type: 'error', description: 'disk on fire' }),
    );
    dispose();
  });

  it('hands a locked vault over to the caller', async () => {
    mockCommands({
      list_entries: () => {
        throw appError('Locked');
      },
    });
    const onLocked = vi.fn();
    const { dispose } = mount(onLocked);
    await settle();

    expect(onLocked).toHaveBeenCalledOnce();
    dispose();
  });
});
