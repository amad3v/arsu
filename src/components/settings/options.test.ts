import { describe, expect, it } from 'vitest';

import {
  applySettingsUpdate,
  CLIPBOARD_CLEAR_PRESETS,
  clipboardClearChoices,
  minutesLabel,
  parseAutoLockMinutes,
  parseClipboardClearSeconds,
  secondsLabel,
} from './options';

describe('clipboardClearChoices', () => {
  it('offers the presets', () => {
    expect(clipboardClearChoices(20)).toEqual(CLIPBOARD_CLEAR_PRESETS);
  });

  it('adds a value set outside the presets, in order, so the menu can show it', () => {
    expect(clipboardClearChoices(25)).toEqual([10, 15, 20, 25, 30, 45, 60]);
  });
});

describe('labels', () => {
  it('pluralise', () => {
    expect(minutesLabel(1)).toBe('1 minute');
    expect(minutesLabel(5)).toBe('5 minutes');
    expect(secondsLabel(20)).toBe('20 seconds');
  });
});

describe('parseAutoLockMinutes', () => {
  it('accepts only the delays the backend accepts', () => {
    expect(parseAutoLockMinutes('5')).toBe(5);
    expect(parseAutoLockMinutes('30')).toBe(30);
    expect(parseAutoLockMinutes('3')).toBeNull();
    expect(parseAutoLockMinutes('five')).toBeNull();
  });
});

describe('parseClipboardClearSeconds', () => {
  it('accepts whole seconds from 10 to 60', () => {
    expect(parseClipboardClearSeconds('10')).toBe(10);
    expect(parseClipboardClearSeconds('60')).toBe(60);
    expect(parseClipboardClearSeconds('9')).toBeNull();
    expect(parseClipboardClearSeconds('61')).toBeNull();
    expect(parseClipboardClearSeconds('12.5')).toBeNull();
    expect(parseClipboardClearSeconds('')).toBeNull();
  });
});

describe('applySettingsUpdate', () => {
  const settings = { theme: 'system', autoLockMinutes: 5, clipboardClearSeconds: 20 } as const;

  it('replaces the fields given, and leaves absent or null ones', () => {
    expect(applySettingsUpdate(settings, { autoLockMinutes: 10, theme: null })).toEqual({
      theme: 'system',
      autoLockMinutes: 10,
      clipboardClearSeconds: 20,
    });
  });
});
