import { Dialog } from '@ark-ui/solid/dialog';
import { Show } from 'solid-js';
import { Portal } from 'solid-js/web';

import { createBackDismiss } from '@cpt/back-dismiss';
import { scrollFade } from '@cpt/scroll-fade';
import { SheetModal } from '@cpt/sheet';
import { isTouchUi } from '@cpt/touch-ui';

import './modal.css';

import type { ModalProps } from '@app-types/ui';
import type { ParentComponent } from 'solid-js';

/** Read once: a phone doesn't change interface. */
const TOUCH_UI = isTouchUi();

/**
 * A controlled Ark dialog. Its content mounts when it opens and unmounts once
 * the exit animation has played, so state kept inside it resets on every open.
 * A body too tall for the window scrolls under the title, which stays put.
 *
 * In the touch interface it is a bottom sheet, or with `page` a full-screen
 * page that slides up from the bottom; Android's Back button closes either.
 */
export const Modal: ParentComponent<ModalProps> = (props) => (
  <Show when={TOUCH_UI && props.page !== true} fallback={<CentredOrPage {...props} />}>
    <SheetModal {...props} />
  </Show>
);

/** The desktop's centred dialog, or the touch interface's full-screen page. */
const CentredOrPage: ParentComponent<ModalProps> = (props) => {
  createBackDismiss(
    () => props.open,
    () => props.onClose(),
    TOUCH_UI,
  );

  return (
    <Dialog.Root
      open={props.open}
      onOpenChange={(details) => {
        if (!details.open) props.onClose();
      }}
      initialFocusEl={props.initialFocusEl}
      finalFocusEl={props.finalFocusEl}
      onExitComplete={() => props.onExitComplete?.()}
      lazyMount
      unmountOnExit
    >
      <Portal>
        <Dialog.Backdrop class={'modal-backdrop bg-black/60 inset-0 fixed z-40'} />

        <Dialog.Positioner
          class={
            TOUCH_UI
              ? 'flex inset-0 fixed z-50'
              : 'p-4 flex items-center inset-0 justify-center fixed z-50'
          }
        >
          <Dialog.Content
            class={
              TOUCH_UI
                ? 'modal-page text-text bg-bg-app flex flex-col h-full w-full'
                : `modal-content text-text border border-border rounded-xl bg-bg-app flex flex-col max-h-[90vh] w-full shadow-2xl ${
                    props.wide ? 'max-w-3xl' : 'max-w-lg'
                  }`
            }
          >
            <div
              class={
                TOUCH_UI
                  ? 'px-4 pb-3 pt-4 flex gap-3 items-start justify-between'
                  : 'px-6 pb-4 pt-6 flex gap-4 items-start justify-between'
              }
            >
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
                  TOUCH_UI
                    ? 'text-text-muted -mr-2 -mt-2 rounded-full flex shrink-0 size-12 transition-colors items-center justify-center active:bg-bg-pressed'
                    : 'text-text-muted p-1 rounded-md flex shrink-0 transition-colors hover:text-text hover:bg-bg-hover'
                }
              >
                <i class={'i-ph-x size-5'} aria-hidden={'true'} />
              </Dialog.CloseTrigger>
            </div>

            <div
              ref={scrollFade}
              class={
                TOUCH_UI
                  ? 'scroll-fade px-4 pb-6 flex-1 min-h-0 overflow-y-auto'
                  : 'scroll-fade px-6 pb-6 min-h-0 overflow-y-auto'
              }
            >
              {props.children}
            </div>
          </Dialog.Content>
        </Dialog.Positioner>
      </Portal>
    </Dialog.Root>
  );
};
