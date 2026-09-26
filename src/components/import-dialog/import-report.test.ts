import { describe, expect, it } from 'vitest';

import { importReport } from './import-report';

import type { ImportSummary } from '@app-types/api';

function summary(changes: Partial<ImportSummary>): ImportSummary {
  return { importedIds: [], skipped: [], duplicates: [], ...changes };
}

const bad = { label: 'Bank (bob)', reason: 'unsupported algorithm MD5' };
const twice = { label: 'GitHub (alice)', reason: 'already in the vault' };

describe('importReport', () => {
  it('counts what was imported', () => {
    expect(importReport(summary({ importedIds: ['a'] })).headline).toBe('Imported 1 entry.');
    expect(importReport(summary({ importedIds: ['a', 'b', 'c'] })).headline).toBe(
      'Imported 3 entries.',
    );
  });

  it('says so when nothing new was imported', () => {
    expect(importReport(summary({ duplicates: [twice] })).headline).toBe(
      'No new entries were imported.',
    );
  });

  it('is only a headline when everything was imported', () => {
    expect(importReport(summary({ importedIds: ['a', 'b'] }))).toEqual({
      headline: 'Imported 2 entries.',
      warning: null,
      sections: [],
    });
  });

  it('lists skipped entries with their reasons, and warns to keep the backup', () => {
    const report = importReport(summary({ importedIds: ['a'], skipped: [bad] }));

    expect(report.warning).toBe(
      "1 entry couldn't be imported. Keep the original backup until you've added it another way.",
    );
    expect(report.sections).toHaveLength(1);
    expect(report.sections[0]).toMatchObject({ title: 'Not imported (1)', entries: [bad] });
  });

  it('lists duplicates, which need no warning', () => {
    const report = importReport(summary({ importedIds: ['a'], duplicates: [twice, twice] }));

    expect(report.warning).toBeNull();
    expect(report.sections.map((section) => section.title)).toEqual(['Already in your vault (2)']);
    expect(report.sections[0].entries).toEqual([twice, twice]);
  });

  it('puts skipped entries before duplicates', () => {
    const report = importReport(summary({ skipped: [bad, bad], duplicates: [twice] }));

    expect(report.sections.map((section) => section.title)).toEqual([
      'Not imported (2)',
      'Already in your vault (1)',
    ]);
    expect(report.warning).toMatch(
      /^2 entries couldn't be imported\. .* added them another way\.$/,
    );
  });
});
