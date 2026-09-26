// The dashboard's keyboard path: the search field keeps focus
// while the arrow keys move a highlighted row, Enter copies its code and
// Escape clears the search; "/" or Ctrl+F comes back to the search, Ctrl+L locks.
// A click on a row selects it the same way; a double-click copies its code.

/** The parts of a KeyboardEvent the shortcuts depend on. */
export type KeyInput = Pick<
  KeyboardEvent,
  'key' | 'altKey' | 'ctrlKey' | 'metaKey' | 'shiftKey' | 'isComposing'
>;

export type SearchKeyAction = 'next' | 'previous' | 'copy' | 'clear';

/** What a key pressed in the search field does to the list, if anything. */
export function searchKeyAction(event: KeyInput): SearchKeyAction | null {
  if (event.isComposing || event.altKey || event.ctrlKey || event.metaKey) return null;

  switch (event.key) {
    case 'ArrowDown':
      return 'next';
    case 'ArrowUp':
      return 'previous';
    case 'Enter':
      return 'copy';
    case 'Escape':
      return 'clear';
    default:
      return null;
  }
}

export type ShortcutAction = 'focus-search' | 'lock';

/**
 * The dashboard-wide shortcut for a key, if any. "/" is left alone while the
 * user types in a text field, where it is just a character.
 */
export function shortcutAction(event: KeyInput, inTextField: boolean): ShortcutAction | null {
  if (event.isComposing || event.altKey || event.metaKey) return null;

  if (event.ctrlKey) {
    if (event.shiftKey) return null;
    switch (event.key.toLowerCase()) {
      case 'f':
        return 'focus-search';
      case 'l':
        return 'lock';
      default:
        return null;
    }
  }

  return event.key === '/' && !inTextField ? 'focus-search' : null;
}

/** The highlighted row: the one the user moved to while it is still listed, else the first. */
export function currentHighlight(ids: readonly string[], chosen: string | null): string | null {
  if (chosen !== null && ids.includes(chosen)) return chosen;
  return ids.length > 0 ? ids[0] : null;
}

/** The row after (`step` 1) or before (-1) `current`, stopping at the ends of the list. */
export function stepHighlight(
  ids: readonly string[],
  current: string | null,
  step: 1 | -1,
): string | null {
  if (ids.length === 0) return null;

  const index = current === null ? -1 : ids.indexOf(current);
  if (index === -1) return step === 1 ? ids[0] : ids[ids.length - 1];

  const next = Math.min(ids.length - 1, Math.max(0, index + step));
  return ids[next];
}
