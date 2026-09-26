import { onCleanup, onMount } from 'solid-js';

export interface KeepFocusOptions {
  /** The screen: presses and focus outside it (a menu's popup) are left alone. */
  root: () => HTMLElement | undefined;
  /** The element that keeps the focus. */
  anchor: () => HTMLElement | undefined;
  /** Elements, besides the anchor, a press may give the focus to. */
  allow: (target: Element) => boolean;
}

/**
 * Keeps the focus on `anchor` whatever the pointer does on the screen. A
 * press elsewhere (the background, a label, a toggle) doesn't take the focus
 * — its default action is prevented, its click still fires — and brings it
 * back if it had left; a focus that follows a press (a menu handing it back
 * to its trigger as it closes) is sent back too.
 *
 * The keyboard is left alone, so Tab still reaches every control.
 */
export function keepFocus(options: KeepFocusOptions): void {
  onMount(() => {
    // Whether the latest input was the pointer's rather than the keyboard's.
    let pointer = false;

    const isHeld = (target: EventTarget | null): target is Element =>
      target instanceof Element &&
      options.root()?.contains(target) === true &&
      target !== options.anchor() &&
      !options.allow(target);

    const onMouseDown = (event: MouseEvent) => {
      pointer = true;
      if (!isHeld(event.target)) return;
      event.preventDefault();
      options.anchor()?.focus();
    };
    const onKeyDown = () => {
      pointer = false;
    };
    const onFocusIn = (event: FocusEvent) => {
      if (pointer && isHeld(event.target)) options.anchor()?.focus();
    };

    // Capture: seen before any control's own handlers.
    document.addEventListener('mousedown', onMouseDown, true);
    document.addEventListener('keydown', onKeyDown, true);
    document.addEventListener('focusin', onFocusIn, true);
    onCleanup(() => {
      document.removeEventListener('mousedown', onMouseDown, true);
      document.removeEventListener('keydown', onKeyDown, true);
      document.removeEventListener('focusin', onFocusIn, true);
    });
  });
}
