import { onMount } from 'solid-js';

/**
 * A `ref` that focuses its element once the component has mounted. For the
 * outcome that replaces a form in a dialog ("Imported 3 entries."): the
 * control that had focus is gone, and moving focus to the outcome makes a
 * screen reader read it. Give a non-interactive element `tabIndex={-1}`.
 *
 * Call it in the component body, not in JSX.
 */
export function createFocusOnMount(): (element: HTMLElement) => void {
  let target: HTMLElement | undefined;
  onMount(() => target?.focus());
  return (element) => {
    target = element;
  };
}
