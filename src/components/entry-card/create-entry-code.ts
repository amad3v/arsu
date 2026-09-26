import { createEffect, untrack } from 'solid-js';
import { createStore } from 'solid-js/store';

import { getCurrentCode } from '@api';
import { errorMessage, isAppError } from '@api/lib';

import { expiryTime, isDue, RETRY_AFTER_MS } from './code-timing';

import type { CodeState } from './code-timing';
import type { EntrySummary } from '@app-types/api';
import type { Accessor } from 'solid-js';

export interface EntryCodeOptions {
  /** The shared clock. A TOTP code is fetched again once it passes the code's expiry. */
  now: Accessor<number>;
  /** The entry no longer exists (deleted meanwhile): refresh the list. */
  onMissing: () => void;
  /** The vault is locked. */
  onLocked: () => void;
}

/** An entry with its code. Lives as long as the entry is in the list, whether shown or filtered out. */
export interface EntryCode {
  readonly entry: EntrySummary;
  /** Reactive. */
  readonly state: Readonly<CodeState>;
  /**
   * Fetches the code now. For HOTP this uses a code up (the backend advances
   * the counter), so call it only when the user asks for a code.
   */
  generate: () => Promise<void>;
}

/**
 * An entry's code. TOTP: fetched at once, then again each time it expires, as
 * the backend's `expiresInSeconds` says. HOTP: fetched only by `generate`, and
 * kept until the next call.
 */
export function createEntryCode(entry: EntrySummary, options: EntryCodeOptions): EntryCode {
  const [state, setState] = createStore<CodeState>({
    code: null,
    nextCode: null,
    expiresAt: null,
    refreshAt: null,
    error: null,
    busy: false,
  });

  async function generate() {
    // One request at a time: for HOTP, a second one would use up another code.
    if (untrack(() => state.busy)) return;

    const requestedAt = Date.now();
    setState('busy', true);

    try {
      const response = await getCurrentCode(entry.id);
      const expiresAt =
        response.expiresInSeconds === null
          ? null
          : expiryTime(requestedAt, response.expiresInSeconds);
      setState({
        code: response.code,
        nextCode: response.nextCode,
        expiresAt,
        refreshAt: expiresAt,
        error: null,
        busy: false,
      });
    } catch (err) {
      setState({ refreshAt: Date.now() + RETRY_AFTER_MS, busy: false });
      if (isAppError(err, 'EntryNotFound')) {
        options.onMissing();
      } else if (isAppError(err, 'Locked')) {
        options.onLocked();
      } else {
        setState('error', errorMessage(err));
      }
    }
  }

  if (entry.otpType === 'totp') {
    createEffect(() => {
      if (isDue(state, options.now())) void generate();
    });
  }

  return { entry, state, generate };
}
