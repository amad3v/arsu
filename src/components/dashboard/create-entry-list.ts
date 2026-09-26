import { batch, untrack } from 'solid-js';
import { createStore, reconcile } from 'solid-js/store';

import { listEntries } from '@api';
import { errorMessage, isAppError } from '@api/lib';
import { toaster } from '@cpt/toaster';

import type { EntrySummary } from '@app-types/api';

export interface EntryListState {
  /** 'loading' only until the first answer; later refreshes keep the list on screen. */
  status: 'loading' | 'ready' | 'failed';
  /**
   * The active entries, in the order they were added. A refresh reconciles by
   * id, so an unchanged entry keeps its object and its row keeps its state
   * (an HOTP code on screen, a TOTP code's schedule).
   */
  entries: EntrySummary[];
  /** Why the list couldn't be loaded, while `status` is 'failed'. */
  error: string | null;
}

export interface EntryList {
  /** Reactive. */
  readonly state: Readonly<EntryListState>;
  /** Loads the list again. A failure after the first load is reported and leaves the list as it was. */
  refresh: () => Promise<void>;
}

export interface EntryListOptions {
  /** The vault is locked. */
  onLocked: () => void;
}

/** The vault's entries, loaded at once. */
export function createEntryList(options: EntryListOptions): EntryList {
  const [state, setState] = createStore<EntryListState>({
    status: 'loading',
    entries: [],
    error: null,
  });
  // Only the latest refresh may write: an older answer arriving late is stale.
  let latest = 0;

  async function refresh() {
    const request = ++latest;
    const status = () => untrack(() => state.status);
    if (status() === 'failed') setState({ status: 'loading', error: null });

    try {
      const entries = await listEntries();
      if (request !== latest) return;
      batch(() => {
        setState('entries', reconcile(entries, { key: 'id' }));
        setState({ status: 'ready', error: null });
      });
    } catch (err) {
      if (request !== latest) return;
      if (isAppError(err, 'Locked')) {
        options.onLocked();
      } else if (status() === 'ready') {
        toaster.create({
          type: 'error',
          title: "The entry list couldn't be refreshed",
          description: errorMessage(err),
        });
      } else {
        setState({ status: 'failed', error: errorMessage(err) });
      }
    }
  }

  void refresh();

  return { state, refresh };
}
