import { render, screen } from '@solidjs/testing-library';
import { describe, expect, it } from 'vitest';

import { createFocusOnMount } from '.';

import type { Component } from 'solid-js';

describe('createFocusOnMount', () => {
  it('focuses its element once the component mounts', () => {
    const Outcome: Component = () => {
      const focusHere = createFocusOnMount();
      return (
        <p ref={focusHere} tabIndex={-1}>
          {'Imported 3 entries.'}
        </p>
      );
    };

    render(() => <Outcome />);

    expect(document.activeElement).toBe(screen.getByText('Imported 3 entries.'));
  });
});
