import { fireEvent, render } from '@solidjs/testing-library';
import { describe, expect, it } from 'vitest';

import { scrollFade } from '.';

/** A region of `clientHeight` showing content of `scrollHeight` (jsdom has no layout). */
function renderRegion(clientHeight: number, scrollHeight: number): HTMLDivElement {
  const { container } = render(() => (
    <div
      ref={(element) => {
        Object.defineProperties(element, {
          clientHeight: { value: clientHeight },
          scrollHeight: { value: scrollHeight },
        });
        scrollFade(element);
      }}
      class={'scroll-fade'}
    />
  ));
  return container.firstElementChild as HTMLDivElement;
}

const edges = (region: HTMLElement) => ({
  above: region.hasAttribute('data-more-above'),
  below: region.hasAttribute('data-more-below'),
});

describe('scrollFade', () => {
  it('marks no edge when everything fits', () => {
    expect(edges(renderRegion(300, 300))).toEqual({ above: false, below: false });
  });

  it('marks the edges with more content past them as the region scrolls', () => {
    const region = renderRegion(300, 1000);
    expect(edges(region)).toEqual({ above: false, below: true });

    region.scrollTop = 350;
    fireEvent.scroll(region);
    expect(edges(region)).toEqual({ above: true, below: true });

    region.scrollTop = 700;
    fireEvent.scroll(region);
    expect(edges(region)).toEqual({ above: true, below: false });
  });
});
