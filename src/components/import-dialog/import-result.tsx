import { For, Show } from 'solid-js';

import { createFocusOnMount } from '@cpt/focus-on-mount';
import { scrollFade } from '@cpt/scroll-fade';

import type { ImportReport } from './import-report';
import type { Component } from 'solid-js';

export interface ImportResultProps {
  report: ImportReport;
  onDone: () => void;
  /**
   * Shown in the touch interface's sheet, which a tap outside it closes: a
   * key beside the headline, and no Done button.
   */
  inSheet?: boolean;
}

/**
 * What the import did, shown in the dialog until the user dismisses it, so
 * that no skipped account goes unnoticed.
 */
export const ImportResult: Component<ImportResultProps> = (props) => {
  const focusHeadline = createFocusOnMount();

  return (
    <div class={'space-y-4'}>
      <h3
        ref={focusHeadline}
        tabIndex={-1}
        class={'text-base text-text font-semibold flex gap-3 items-center focus:outline-none'}
        classList={{ 'text-lg': props.inSheet === true }}
      >
        <Show when={props.inSheet}>
          <i class={'i-ph-key-bold text-primary shrink-0 size-6'} aria-hidden={'true'} />
        </Show>
        {props.report.headline}
      </h3>

      <Show when={props.report.warning}>
        {(warning) => (
          <p class={'error flex gap-2 items-start'}>
            <i class={'i-ph-warning mt-0.5 shrink-0 size-4'} aria-hidden={'true'} />
            {warning()}
          </p>
        )}
      </Show>

      <For each={props.report.sections}>
        {(section) => (
          <section class={'space-y-2'}>
            <h4 class={'text-sm text-text font-medium'}>{section.title}</h4>
            <p class={'subtle'}>{section.description}</p>
            <ul
              ref={scrollFade}
              class={
                'scroll-fade border border-border rounded-md max-h-48 overflow-y-auto divide-border divide-y'
              }
            >
              <For each={section.entries}>
                {(entry) => (
                  <li class={'text-sm px-3 py-2'}>
                    <span class={'text-text font-medium block'}>{entry.label}</span>
                    <span class={'text-text-muted block'}>{entry.reason}</span>
                  </li>
                )}
              </For>
            </ul>
          </section>
        )}
      </For>

      <Show when={props.inSheet !== true}>
        <div class={'flex justify-end'}>
          <button class={'btn-primary'} onClick={() => props.onDone()} type={'button'}>
            {'Done'}
          </button>
        </div>
      </Show>
    </div>
  );
};
