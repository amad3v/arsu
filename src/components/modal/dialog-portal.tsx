import { usePresenceContext } from '@ark-ui/solid/presence';
import { Show } from 'solid-js';
import { Portal } from 'solid-js/web';

import type { ParentComponent } from 'solid-js';

/**
 * An Ark dialog's portal, in the document only while the dialog is open or
 * playing its exit animation, as the dialog's own parts are (it reads the
 * same presence). A portal kept while closed would be an empty element in
 * the body: a dialog opening in the meantime hides every other element
 * from assistive technology, that one included, and this dialog, once
 * opened from inside it, would stay hidden in it.
 *
 * Goes inside `Dialog.Root`, around the backdrop and positioner.
 */
export const DialogPortal: ParentComponent = (props) => {
  const presence = usePresenceContext();
  return (
    <Show when={presence().unmounted !== true}>
      <Portal>{props.children}</Portal>
    </Show>
  );
};
