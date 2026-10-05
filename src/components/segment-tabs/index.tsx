import { Tabs } from '@ark-ui/solid/tabs';
import { For } from 'solid-js';

import './segment-tabs.css';

import type { JSX } from 'solid-js';

export interface SegmentTab<T extends string> {
  value: T;
  label: string;
  /** The panel's content, created once: every panel stays mounted. */
  content: () => JSX.Element;
}

export interface SegmentTabsProps<T extends string> {
  /** The tab list's accessible name. */
  label: string;
  tabs: readonly SegmentTab<T>[];
  value: T;
  onValueChange: (value: T) => void;
  /**
   * Fill the height the parent (a flex column) gives, and no more: a panel
   * taller than that lays itself out to scroll inside, as the page doesn't.
   */
  fill?: boolean;
}

/**
 * Tabs drawn as a segmented control, the selected segment sliding to the
 * chosen tab, over panels that share one area as big as the biggest: a
 * dialog holding them keeps its size as the user switches tabs.
 *
 * The tabs render once (a `<For>` over a static list), so only the
 * indicator's position changes, which is what lets it slide.
 */
export function SegmentTabs<T extends string>(props: SegmentTabsProps<T>): JSX.Element {
  return (
    <Tabs.Root
      value={props.value}
      onValueChange={(details) => {
        const chosen = props.tabs.find((tab) => tab.value === details.value);
        if (chosen !== undefined) props.onValueChange(chosen.value);
      }}
      class={props.fill === true ? 'flex flex-col flex-1 gap-4 min-h-0' : 'flex flex-col gap-4'}
    >
      <Tabs.List
        aria-label={props.label}
        class={'p-1 rounded-lg bg-segment-track inline-flex gap-1 self-start relative isolate'}
      >
        {/* The selected tab's background: one element, sliding from tab to tab. */}
        <Tabs.Indicator class={'segment-indicator rounded-md bg-segment-thumb shadow-sm'} />
        <For each={props.tabs}>
          {(tab) => (
            <Tabs.Trigger
              value={tab.value}
              class={
                'text-sm text-text-muted font-medium px-3 py-1.5 outline-none rounded-md transition-colors relative z-1 data-[selected]:text-text hover:text-text'
              }
            >
              {tab.label}
            </Tabs.Trigger>
          )}
        </For>
      </Tabs.List>

      {/*
        Every panel stays in the layout (see segment-tabs.css), so the panels
        not shown are hidden by the stylesheet rather than by Ark's `hidden`
        attribute: the reset's `[hidden] { display: none !important }` would
        take them out of it.
      */}
      <div
        class={
          props.fill === true
            ? 'segment-panels grid-rows-[minmax(0,1fr)] flex-1 min-h-0'
            : 'segment-panels flex-1'
        }
      >
        <For each={props.tabs}>
          {(tab) => (
            <Tabs.Content value={tab.value} hidden={false}>
              {tab.content()}
            </Tabs.Content>
          )}
        </For>
      </div>
    </Tabs.Root>
  );
}
