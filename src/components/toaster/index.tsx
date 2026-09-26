import { createToaster, Toast, Toaster } from '@ark-ui/solid/toast';
import { Show } from 'solid-js';

import { toastIcon } from './icons';

import './toaster.css';

import type { Component } from 'solid-js';

/** Create toasts with `toaster.create({ type, title, description })`. */
export const toaster = createToaster({
  placement: 'bottom-end',
  // Stacked, each toast above the previous one, so none hides another.
  overlap: false,
  gap: 12,
  // Long enough for the exit transition in toaster.css to finish.
  removeDelay: 250,
});

export const GlobalToaster: Component = () => (
  <Toaster toaster={toaster}>
    {(toast) => (
      <Toast.Root
        class={
          'toast text-text p-4 pr-3 border border-border rounded-lg bg-bg-card flex gap-3 max-w-[calc(100vw-2rem)] w-80 shadow-lg items-start focus-visible:outline-none data-[type=error]:border-error-border data-[type=success]:border-success-border focus-visible:ring-2 focus-visible:ring-primary'
        }
      >
        <i class={`${toastIcon(toast().type)} mt-0.5 shrink-0 size-5`} aria-hidden={'true'} />

        <div class={'flex-1 min-w-0'}>
          <Toast.Title class={'font-semibold'}>{toast().title}</Toast.Title>
          <Show when={toast().description}>
            {(description) => (
              <Toast.Description class={'text-sm text-text-muted mt-1'}>
                {description()}
              </Toast.Description>
            )}
          </Show>
        </div>

        <Toast.CloseTrigger
          aria-label={'Dismiss notification'}
          class={'text-text-muted p-0.5 rounded flex shrink-0 transition-colors hover:text-text'}
        >
          <i class={'i-ph-x size-4'} aria-hidden={'true'} />
        </Toast.CloseTrigger>
      </Toast.Root>
    )}
  </Toaster>
);
