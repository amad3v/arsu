import type { ImportFormat } from '@app-types/api';

/** What a picked backup is, by the format its content shows. */
export const IMPORT_FORMAT_LABEL: Record<ImportFormat, string> = {
  aegis: 'Aegis vault export',
  twofas: '2FAS backup',
};
