import { describe, expect, it } from 'vitest';

import { currentHighlight, searchKeyAction, shortcutAction, stepHighlight } from './keyboard';

import type { KeyInput } from './keyboard';

function key(value: string, modifiers: Partial<Omit<KeyInput, 'key'>> = {}): KeyInput {
  return {
    key: value,
    altKey: false,
    ctrlKey: false,
    metaKey: false,
    shiftKey: false,
    isComposing: false,
    ...modifiers,
  };
}

const ids = ['a', 'b', 'c'];

describe('currentHighlight', () => {
  it('is the first row until the user moves', () => {
    expect(currentHighlight(ids, null)).toBe('a');
  });

  it('stays on the row the user moved to', () => {
    expect(currentHighlight(ids, 'b')).toBe('b');
  });

  it('falls back to the first row when that row is no longer listed', () => {
    expect(currentHighlight(ids, 'gone')).toBe('a');
  });

  it('is nothing for an empty list', () => {
    expect(currentHighlight([], 'a')).toBeNull();
  });
});

describe('stepHighlight', () => {
  it('moves down and up', () => {
    expect(stepHighlight(ids, 'a', 1)).toBe('b');
    expect(stepHighlight(ids, 'c', -1)).toBe('b');
  });

  it('stops at either end', () => {
    expect(stepHighlight(ids, 'c', 1)).toBe('c');
    expect(stepHighlight(ids, 'a', -1)).toBe('a');
  });

  it('enters the list at the matching end', () => {
    expect(stepHighlight(ids, null, 1)).toBe('a');
    expect(stepHighlight(ids, null, -1)).toBe('c');
    expect(stepHighlight(ids, 'gone', -1)).toBe('c');
  });

  it('is nothing for an empty list', () => {
    expect(stepHighlight([], 'a', 1)).toBeNull();
  });
});

describe('searchKeyAction', () => {
  it('maps the arrow keys, Enter and Escape', () => {
    expect(searchKeyAction(key('ArrowDown'))).toBe('next');
    expect(searchKeyAction(key('ArrowUp'))).toBe('previous');
    expect(searchKeyAction(key('Enter'))).toBe('copy');
    expect(searchKeyAction(key('Escape'))).toBe('clear');
  });

  it('leaves other keys to the field', () => {
    expect(searchKeyAction(key('a'))).toBeNull();
    expect(searchKeyAction(key('ArrowLeft'))).toBeNull();
  });

  it('ignores keys with Ctrl, Alt or Meta, and keys that compose text (IME)', () => {
    expect(searchKeyAction(key('Enter', { ctrlKey: true }))).toBeNull();
    expect(searchKeyAction(key('ArrowDown', { altKey: true }))).toBeNull();
    expect(searchKeyAction(key('Enter', { metaKey: true }))).toBeNull();
    expect(searchKeyAction(key('Enter', { isComposing: true }))).toBeNull();
  });
});

describe('shortcutAction', () => {
  it('focuses the search on "/" outside a text field', () => {
    expect(shortcutAction(key('/'), false)).toBe('focus-search');
  });

  it('types "/" into a text field', () => {
    expect(shortcutAction(key('/'), true)).toBeNull();
  });

  it('focuses the search on Ctrl+F, even in a text field', () => {
    expect(shortcutAction(key('f', { ctrlKey: true }), true)).toBe('focus-search');
    expect(shortcutAction(key('F', { ctrlKey: true }), false)).toBe('focus-search');
  });

  it('locks on Ctrl+L', () => {
    expect(shortcutAction(key('l', { ctrlKey: true }), true)).toBe('lock');
  });

  it('ignores other combinations', () => {
    expect(shortcutAction(key('f'), false)).toBeNull();
    expect(shortcutAction(key('l', { ctrlKey: true, shiftKey: true }), false)).toBeNull();
    expect(shortcutAction(key('f', { ctrlKey: true, altKey: true }), false)).toBeNull();
    expect(shortcutAction(key('/', { isComposing: true }), false)).toBeNull();
  });
});
