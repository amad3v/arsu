/**
 * The icon, with its colour, for a toast of `type`. Colour is never the only
 * signal: each type has its own shape too.
 */
export function toastIcon(type: string | undefined): string {
  switch (type) {
    case 'success':
      return 'i-ph-check-circle text-success-text';
    case 'error':
      return 'i-ph-warning-circle text-error-text';
    case 'warning':
      return 'i-ph-warning text-error-text';
    default:
      return 'i-ph-info text-info-text';
  }
}
