import { describe, expect, it } from 'vitest';

import { isTextEntry } from '.';

describe('isTextEntry', () => {
  function input(type: string): HTMLInputElement {
    const element = document.createElement('input');
    element.type = type;
    return element;
  }

  // Editable content isn't covered: jsdom doesn't implement isContentEditable.
  it('is true for text inputs and text areas', () => {
    expect(isTextEntry(document.createElement('textarea'))).toBe(true);
    for (const type of ['text', 'password', 'search', 'email', 'url']) {
      expect(isTextEntry(input(type))).toBe(true);
    }
  });

  it('is false for everything else', () => {
    expect(isTextEntry(input('checkbox'))).toBe(false);
    expect(isTextEntry(input('radio'))).toBe(false);
    // Elements other than inputs: falsy, as jsdom has no isContentEditable to read.
    expect(isTextEntry(document.createElement('button'))).toBeFalsy();
    expect(isTextEntry(document.body)).toBeFalsy();
    expect(isTextEntry(null)).toBe(false);
  });
});
