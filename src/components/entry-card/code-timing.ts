// When a code is shown, previewed and fetched again. Everything is driven by
// the backend's `expiresInSeconds`; no period is ever assumed.

/** How close to expiry, in seconds, the countdown warns and the next code is previewed. */
export const URGENT_SECONDS = 5;

/** How long to wait before fetching again after a failed fetch. */
export const RETRY_AFTER_MS = 5_000;

/** An entry's code, as its row shows it. */
export interface CodeState {
  /** The last code fetched; null until a fetch succeeds. */
  code: string | null;
  /** TOTP: the code that follows `code`. Null for HOTP. */
  nextCode: string | null;
  /** TOTP: when `code` expires, in ms since the epoch. Null for HOTP. */
  expiresAt: number | null;
  /** TOTP: when to fetch again, in ms since the epoch; null means now. */
  refreshAt: number | null;
  /** Why the last fetch failed, for the user. */
  error: string | null;
  /** A fetch is in flight. */
  busy: boolean;
}

/**
 * When a code expires, in ms since the epoch. The backend reads the same
 * system clock in whole seconds, so its time steps end on whole seconds.
 * Flooring the time the request was sent can therefore make the expiry early
 * (a harmless fetch of the same code) but never late (showing an expired one).
 */
export function expiryTime(requestedAt: number, expiresInSeconds: number): number {
  return (Math.floor(requestedAt / 1000) + expiresInSeconds) * 1000;
}

/** Whole seconds left, as the countdown shows them. */
export function secondsLeft(expiresAt: number, now: number): number {
  return Math.max(0, Math.ceil((expiresAt - now) / 1000));
}

export function isUrgent(seconds: number): boolean {
  return seconds <= URGENT_SECONDS;
}

/** How far into its time step a code is, in ms: where the countdown bar starts draining from. */
export function elapsedInStep(expiresAt: number, periodSeconds: number, now: number): number {
  const periodMs = periodSeconds * 1000;
  return Math.min(periodMs, Math.max(0, periodMs - (expiresAt - now)));
}

/** A TOTP code is due to be fetched: never fetched, expired, or due for a retry. */
export function isDue(state: Readonly<CodeState>, now: number): boolean {
  return !state.busy && (state.refreshAt === null || now >= state.refreshAt);
}

/**
 * The code to show and to copy. A TOTP code past its `expiresAt` is never
 * shown as-is, but `nextCode` is exactly the code for the step that just
 * started, so it is promoted in its place while the background refetch (kicked
 * off by the same `expiresAt`, see `isDue`) resynchronizes. This keeps the row's
 * code button populated, so it never needs to blank out or disable for the
 * round trip. Before the first fetch, or if a fetch failed with no `nextCode`
 * yet, this is null.
 */
export function liveCode(state: Readonly<CodeState>, now: number): string | null {
  if (state.expiresAt !== null && now >= state.expiresAt) return state.nextCode;
  return state.code;
}

/**
 * The next code, as a preview in the seconds before the current one expires.
 * Once `expiresAt` passes, that same code becomes the live one (see
 * `liveCode`) and is no longer "upcoming".
 */
export function upcomingCode(state: Readonly<CodeState>, now: number): string | null {
  if (state.expiresAt === null || state.nextCode === null || now >= state.expiresAt) return null;
  return isUrgent(secondsLeft(state.expiresAt, now)) ? state.nextCode : null;
}
