import { Dialog } from '@ark-ui/solid/dialog';
import { For, Show } from 'solid-js';
import { Portal } from 'solid-js/web';

import { createBackDismiss } from '@cpt/back-dismiss';
import { isTouchUi } from '@cpt/touch-ui';

import './sheet.css';

import type { Component, JSX, ParentComponent } from 'solid-js';

export interface SheetProps {
  open: boolean;
  title: string;
  /** Called when the user dismisses the sheet: a tap outside it, Escape or Back. */
  onClose: () => void;
}

/**
 * A bottom sheet: the touch interface's menu. It rises from the bottom edge,
 * within reach of a thumb, holds a short list of large rows, and closes on a
 * tap outside it or Android's Back button.
 */
export const Sheet: ParentComponent<SheetProps> = (props) => {
  createBackDismiss(
    () => props.open,
    () => props.onClose(),
    isTouchUi(),
  );

  return (
    <Dialog.Root
      open={props.open}
      onOpenChange={(details) => {
        if (!details.open) props.onClose();
      }}
      lazyMount
      unmountOnExit
    >
      <Portal>
        <Dialog.Backdrop class={'sheet-backdrop bg-black/50 inset-0 fixed z-40'} />
        <Dialog.Positioner class={'flex items-end inset-0 justify-center fixed z-50'}>
          <Dialog.Content
            class={
              'sheet-content text-text pb-2 rounded-t-2xl bg-bg-card flex flex-col max-h-[85vh] w-full shadow-2xl'
            }
          >
            <span
              class={'mt-2 rounded-full bg-border-strong h-1 w-10 self-center'}
              aria-hidden={'true'}
            />
            <Dialog.Title class={'text-sm text-text-muted font-medium px-5 pb-1 pt-3'}>
              {props.title}
            </Dialog.Title>
            <div class={'overflow-y-auto'}>{props.children}</div>
          </Dialog.Content>
        </Dialog.Positioner>
      </Portal>
    </Dialog.Root>
  );
};

export interface SheetActionProps {
  icon: string;
  label: string;
  /** A second line under the label, saying more. */
  description?: string;
  onSelect: () => void;
  /** A destructive action, in the error colour. */
  danger?: boolean;
}

/** One of a sheet's actions: a full-width row, 56 px tall. */
export const SheetAction: Component<SheetActionProps> = (props) => (
  <button
    type={'button'}
    class={
      'sheet-row text-base px-5 pressable flex gap-4 min-h-14 w-full items-center active:bg-bg-pressed'
    }
    classList={{ 'text-error-text': props.danger === true }}
    onClick={() => props.onSelect()}
  >
    <i
      class={`${props.icon} size-6`}
      classList={{ 'text-text-muted': props.danger !== true }}
      aria-hidden={'true'}
    />
    <span class={'py-2 text-left flex flex-1 flex-col'}>
      <span>{props.label}</span>
      <Show when={props.description}>
        {(description) => <span class={'subtle'}>{description()}</span>}
      </Show>
    </span>
  </button>
);

export interface SheetChoicesProps<T extends string | number> {
  /** The choices' accessible group name, e.g. "Lock when idle for". */
  label: string;
  choices: readonly T[];
  current: T;
  choiceLabel: (value: T) => string;
  onChoose: (value: T) => void;
}

/** A sheet's single choice among values: radio rows, the current one checked. */
export function SheetChoices<T extends string | number>(props: SheetChoicesProps<T>): JSX.Element {
  return (
    <div role={'radiogroup'} aria-label={props.label}>
      <For each={props.choices}>
        {(value) => (
          <button
            type={'button'}
            role={'radio'}
            aria-checked={value === props.current}
            class={
              'sheet-row text-base px-5 pressable flex gap-4 min-h-14 w-full items-center active:bg-bg-pressed'
            }
            onClick={() => props.onChoose(value)}
          >
            <span class={'text-left flex-1'}>{props.choiceLabel(value)}</span>
            <Show when={value === props.current}>
              <i class={'i-ph-check-bold text-primary size-5'} aria-hidden={'true'} />
            </Show>
          </button>
        )}
      </For>
    </div>
  );
}
