import type { Theme } from '@app-types/api';

export const themeIcon: Record<Theme, string> = {
  light: 'i-ph-sun',
  dark: 'i-ph-moon',
  system: 'i-ph-monitor',
};

export const themeLabel: Record<Theme, string> = {
  light: 'Light',
  dark: 'Dark',
  system: 'System',
};
