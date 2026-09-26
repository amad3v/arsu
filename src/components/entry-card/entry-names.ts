import type { EntrySummary } from '@app-types/api';

/** What a row calls the entry: its issuer, or its account when it has none. */
export function entryTitle(entry: EntrySummary): string {
  return entry.issuer ?? entry.accountLabel;
}

/** The line under the title: the account, unless the account is already the title. */
export function entrySubtitle(entry: EntrySummary): string | null {
  return entry.issuer === null ? null : entry.accountLabel;
}

/** The DOM id of the entry's row, for scrolling it into view. */
export function entryRowId(entryId: string): string {
  return `entry-${entryId}`;
}

/** The DOM id of the row's "More actions" menu trigger, to return focus to it once a row dialog closes. */
export function entryActionsId(entryId: string): string {
  return `${entryRowId(entryId)}-actions`;
}
