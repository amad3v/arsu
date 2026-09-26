import { onCleanup } from 'solid-js';

import './scroll-fade.css';

/**
 * Marks which ends of a scrolling element have more content past them
 * (`data-more-above`, `data-more-below`), for `.scroll-fade` to fade those
 * edges: the cue that the region scrolls, as the scrollbars are hidden.
 *
 * Kept up to date as the element scrolls, and as it or its children change
 * size (a list filtered, a window resized). Use it as a ref, inside a
 * component: `<div class={'scroll-fade overflow-y-auto'} ref={scrollFade}>`.
 */
export function scrollFade(element: HTMLElement): void {
  const update = () => {
    const hidden = element.scrollHeight - element.clientHeight;
    // A pixel of slack: fractional sizes can leave the end a hair short.
    element.toggleAttribute('data-more-above', element.scrollTop > 1);
    element.toggleAttribute('data-more-below', element.scrollTop < hidden - 1);
  };

  const resizes = new ResizeObserver(update);
  const observeChildren = () => {
    for (const child of element.children) resizes.observe(child);
  };
  resizes.observe(element);
  observeChildren();

  // Children come and go (a <Show>, a <For>): watch the new ones too.
  const mutations = new MutationObserver(() => {
    observeChildren();
    update();
  });
  mutations.observe(element, { childList: true });

  element.addEventListener('scroll', update, { passive: true });
  update();

  onCleanup(() => {
    element.removeEventListener('scroll', update);
    mutations.disconnect();
    resizes.disconnect();
  });
}
