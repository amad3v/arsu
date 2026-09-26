import type { ImportSummary, SkippedEntry } from '@app-types/api';

/** Entries of the file that were not added, and why. */
export interface ImportReportSection {
  title: string;
  description: string;
  entries: readonly SkippedEntry[];
}

/** What an import did, for the dialog to show until the user dismisses it. */
export interface ImportReport {
  /** "Imported 3 entries." */
  headline: string;
  /**
   * Set when accounts in the file could not be imported: the user must not
   * delete the original backup on the strength of this import.
   */
  warning: string | null;
  /** Skipped, then duplicate entries; a group with no entries is left out. */
  sections: ImportReportSection[];
}

export function importReport(summary: ImportSummary): ImportReport {
  const imported = summary.importedIds.length;
  const skipped = summary.skipped.length;
  const duplicates = summary.duplicates.length;
  const sections: ImportReportSection[] = [];

  if (skipped > 0) {
    sections.push({
      title: `Not imported (${skipped})`,
      description: "This app can't use these entries:",
      entries: summary.skipped,
    });
  }
  if (duplicates > 0) {
    sections.push({
      title: `Already in your vault (${duplicates})`,
      description:
        "These accounts are in your vault already, or appear twice in the file, so they weren't added again:",
      entries: summary.duplicates,
    });
  }

  return {
    headline:
      imported === 0
        ? 'No new entries were imported.'
        : `Imported ${countOf(imported, 'entry', 'entries')}.`,
    warning:
      skipped === 0
        ? null
        : `${countOf(skipped, 'entry', 'entries')} couldn't be imported. Keep the original backup until you've added ${skipped === 1 ? 'it' : 'them'} another way.`,
    sections,
  };
}

function countOf(count: number, one: string, many: string): string {
  return `${count} ${count === 1 ? one : many}`;
}
