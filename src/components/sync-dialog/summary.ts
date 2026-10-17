// How sync results read on screen.

import type { SyncSummary } from '@app-types/api';

/**
 * One sentence for what a sync changed on this device: "Synced with Computer:
 * 2 accounts added, 1 updated." — or that nothing changed.
 */
export function describeSummary(summary: SyncSummary): string {
  const lead = `${summary.paired ? 'Paired' : 'Synced'} with ${summary.peerName}`;
  const changes = [
    summary.added > 0 && `${summary.added} ${summary.added === 1 ? 'account' : 'accounts'} added`,
    summary.updated > 0 && `${summary.updated} updated`,
    summary.deleted > 0 && `${summary.deleted} deleted`,
  ].filter((change) => change !== false);
  return changes.length === 0
    ? `${lead}. Nothing changed here.`
    : `${lead}: ${changes.join(', ')}.`;
}

/**
 * Whether a device is still waiting for its reply: trusted through an
 * invitation's QR code, its own record not arrived yet. The backend dates
 * such a device after any real time (cmd's PENDING_ADDED_AT, u64::MAX),
 * which no real date in seconds comes near.
 */
export function isPending(addedAt: number): boolean {
  return addedAt > Number.MAX_SAFE_INTEGER;
}
