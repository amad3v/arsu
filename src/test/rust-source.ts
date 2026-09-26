// Reads declarations out of the Rust sources, for the tests that pin the
// TypeScript mirror of the IPC contract to the code it mirrors: a change on
// one side then fails a test instead of breaking at runtime.

import { expect } from 'vitest';

/** The body of the first `<prefix> { … }` block in `source`, which must not nest braces. */
export function rustBlock(source: string, prefix: string): string {
  const start = source.indexOf(prefix);
  expect(start, `\`${prefix}\` not found`).toBeGreaterThanOrEqual(0);
  const open = source.indexOf('{', start);
  return source.slice(open + 1, source.indexOf('}', open));
}

/** The comma-separated items of a block body (enum variants, macro arguments), without comments. */
export function rustListItems(body: string): string[] {
  return body
    .split('\n')
    .map((line) => line.replace(/\/\/.*$/, '').trim())
    .filter((line) => line !== '')
    .map((line) => line.replace(/,$/, ''));
}

/** The first capture group of `pattern` in `source`. */
export function rustCapture(source: string, pattern: RegExp): string {
  const match = pattern.exec(source);
  expect(match, `${String(pattern)} not found`).not.toBeNull();
  return match?.[1] ?? '';
}
