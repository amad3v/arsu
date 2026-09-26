import type { ImportFormat } from '@app-types/api';

/** What a picked backup is, by the format its extension says. */
export const IMPORT_FORMAT_LABEL: Record<ImportFormat, string> = {
  aegis: 'Aegis vault export (.json)',
  twofas: '2FAS backup (.2fas)',
};
