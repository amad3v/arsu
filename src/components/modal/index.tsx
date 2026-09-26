import { Dialog } from '@ark-ui/solid/dialog';
import { Show } from 'solid-js';
import { Portal } from 'solid-js/web';

import { scrollFade } from '@cpt/scroll-fade';

import './modal.css';

import type { ModalProps } from '@app-types/ui';
import type { ParentComponent } from 'solid-js';

/**
 * A controlled Ark dialog. Its content mounts when it opens and unmounts once
 * the exit animation has played, so state kept inside it resets on every open.
 * A body too tall for the window scrolls under the title, which stays put.
 */
export const Modal: ParentComponent<ModalProps> = (props) => (
  <Dialog.Root
    open={props.open}
    onOpenChange={(details) => {
      if (!details.open) props.onClose();
    }}
    initialFocusEl={props.initialFocusEl}
    finalFocusEl={props.finalFocusEl}
    lazyMount
    unmountOnExit
  >
    <Portal>
      <Dialog.Backdrop class={'modal-backdrop bg-black/60 inset-0 fixed z-40'} />

      <Dialog.Positioner class={'p-4 flex items-center inset-0 justify-center fixed z-50'}>
        <Dialog.Content
          class={`modal-content text-text border border-border rounded-xl bg-bg-app flex flex-col max-h-[90vh] w-full shadow-2xl ${
            props.wide ? 'max-w-3xl' : 'max-w-lg'
          }`}
        >
          <div class={'px-6 pb-4 pt-6 flex gap-4 items-start justify-between'}>
            <div class={'min-w-0'}>
              <Dialog.Title class={'text-lg text-text font-semibold'}>{props.title}</Dialog.Title>
              <Show when={props.description}>
                {(description) => (
                  <Dialog.Description class={'subtle mt-1'}>{description()}</Dialog.Description>
                )}
              </Show>
            </div>

            <Dialog.CloseTrigger
              aria-label={'Close'}
              class={
                'text-text-muted p-1 rounded-md flex shrink-0 transition-colors hover:text-text hover:bg-bg-hover'
              }
            >
              <i class={'i-ph-x size-5'} aria-hidden={'true'} />
            </Dialog.CloseTrigger>
          </div>

          <div ref={scrollFade} class={'scroll-fade px-6 pb-6 min-h-0 overflow-y-auto'}>
            {props.children}
          </div>
        </Dialog.Content>
      </Dialog.Positioner>
    </Portal>
  </Dialog.Root>
);
