// Search and order of the entry list: sorted by issuer, then account, and
// filtered on both, case-insensitively.

import { entryTitle } from '@cpt/entry-card/entry-names';

import type { EntrySummary } from '@app-types/api';

const collator = new Intl.Collator(undefined, { sensitivity: 'base', numeric: true });

/**
 * By issuer, then account. An entry without an issuer sorts by its account,
 * which is what its row shows as the title.
 */
export function compareEntries(a: EntrySummary, b: EntrySummary): number {
  return (
    collator.compare(entryTitle(a), entryTitle(b)) ||
    collator.compare(a.accountLabel, b.accountLabel)
  );
}

/** The query's words, lower-cased. */
export function searchTerms(query: string): string[] {
  return query
    .toLocaleLowerCase()
    .split(/\s+/)
    .filter((term) => term !== '');
}

/** Every term appears in the issuer or the account ("git work" finds GitHub, work@example.com). */
export function matchesTerms(entry: EntrySummary, terms: readonly string[]): boolean {
  // The newline keeps a term from matching across the issuer/account boundary.
  const text = `${entry.issuer ?? ''}\n${entry.accountLabel}`.toLocaleLowerCase();
  return terms.every((term) => text.includes(term));
}

/** Anything that carries an entry, like a row's EntryCode. */
export interface HasEntry {
  readonly entry: EntrySummary;
}

/** The items to list for `query`: those whose entry matches, in order. */
export function visibleItems<T extends HasEntry>(items: readonly T[], query: string): T[] {
  const terms = searchTerms(query);
  return items
    .filter((item) => matchesTerms(item.entry, terms))
    .sort((a, b) => compareEntries(a.entry, b.entry));
}
