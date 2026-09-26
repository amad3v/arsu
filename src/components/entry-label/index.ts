import type { EntrySummary } from '@app-types/api';

/**
 * How a dialog names an entry: "GitHub (alice)", or just the account name
 * when there is no issuer. Specific enough to tell two accounts at the same
 * service apart.
 */
export function entryLabel(entry: Pick<EntrySummary, 'issuer' | 'accountLabel'>): string {
  return entry.issuer === null ? entry.accountLabel : `${entry.issuer} (${entry.accountLabel})`;
}
