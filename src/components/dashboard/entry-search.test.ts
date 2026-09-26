import { describe, expect, it } from 'vitest';

import { compareEntries, matchesTerms, searchTerms, visibleItems } from './entry-search';

import type { EntrySummary } from '@app-types/api';

function entry(id: string, issuer: string | null, accountLabel: string): EntrySummary {
  return { id, issuer, accountLabel, otpType: 'totp', digits: 6, period: 30 };
}

const github = entry('1', 'GitHub', 'alice');
const githubWork = entry('2', 'GitHub', 'alice@work.example');
const aws = entry('3', 'AWS', 'root');
const noIssuer = entry('4', null, 'bob@example.com');
const zoho = entry('5', 'zoho', 'carol');

const items = [github, zoho, noIssuer, githubWork, aws].map((e) => ({ entry: e }));
const ids = (list: readonly { entry: EntrySummary }[]) => list.map((item) => item.entry.id);

describe('compareEntries', () => {
  it('orders by issuer, case-insensitively, then by account', () => {
    expect([zoho, githubWork, aws, github].sort(compareEntries)).toEqual([
      aws,
      github,
      githubWork,
      zoho,
    ]);
  });

  it('sorts an entry without an issuer by its account, which is its title', () => {
    expect([github, noIssuer, aws].sort(compareEntries)).toEqual([aws, noIssuer, github]);
  });

  it('compares numbers by value', () => {
    const second = entry('a', 'Bank 2', 'x');
    const tenth = entry('b', 'Bank 10', 'x');
    expect([tenth, second].sort(compareEntries)).toEqual([second, tenth]);
  });
});

describe('searchTerms', () => {
  it('lower-cases the query and splits it into words', () => {
    expect(searchTerms('  GitHub   Work ')).toEqual(['github', 'work']);
  });

  it('has no terms for a blank query', () => {
    expect(searchTerms('   ')).toEqual([]);
  });
});

describe('matchesTerms', () => {
  it('matches the issuer or the account, case-insensitively', () => {
    expect(matchesTerms(github, ['git'])).toBe(true);
    expect(matchesTerms(github, ['ALICE'.toLowerCase()])).toBe(true);
    expect(matchesTerms(noIssuer, ['example'])).toBe(true);
    expect(matchesTerms(aws, ['github'])).toBe(false);
  });

  it('needs every term to match', () => {
    expect(matchesTerms(githubWork, ['github', 'work'])).toBe(true);
    expect(matchesTerms(github, ['github', 'work'])).toBe(false);
  });

  it('does not match a term across the issuer and the account', () => {
    // "GitHub" + "alice" must not read as "githubalice".
    expect(matchesTerms(github, ['hubali'])).toBe(false);
  });

  it('matches everything with no terms', () => {
    expect(matchesTerms(aws, [])).toBe(true);
  });
});

describe('visibleItems', () => {
  it('lists every item, sorted, for an empty query', () => {
    expect(ids(visibleItems(items, ''))).toEqual(['3', '4', '1', '2', '5']);
  });

  it('keeps the matching items, sorted', () => {
    expect(ids(visibleItems(items, 'git'))).toEqual(['1', '2']);
    expect(ids(visibleItems(items, 'Alice work'))).toEqual(['2']);
    expect(ids(visibleItems(items, 'nothing'))).toEqual([]);
  });

  it('returns the same item objects, not copies', () => {
    expect(visibleItems(items, 'aws')[0]).toBe(items[4]);
  });
});
