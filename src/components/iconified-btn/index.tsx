import { Tooltip } from '@ark-ui/solid/tooltip';
import { splitProps } from 'solid-js';
import { Portal } from 'solid-js/web';

import { BaseButton } from './base-btn';

import type { IconifiedButtonProps } from '@app-types/ui';
import type { Component } from 'solid-js';

/**
 * An icon-only button with a tooltip. `label` is both the accessible name and
 * the tooltip text, so the two can't disagree.
 */
export const IconifiedButton: Component<IconifiedButtonProps> = (props) => {
  const [local, buttonProps] = splitProps(props, ['icon', 'iconSz', 'label']);

  return (
    // Mounted only while shown: a list of cards would otherwise keep a hidden
    // tooltip in the document for every button.
    <Tooltip.Root openDelay={200} closeDelay={0} lazyMount unmountOnExit>
      <Tooltip.Trigger
        // `triggerProps(buttonProps)` merges the trigger's props with the
        // caller's and chains their event handlers, so a caller's onClick and
        // the tooltip's own handlers both run.
        asChild={(triggerProps) => (
          <BaseButton
            {...triggerProps(buttonProps)}
            icon={local.icon}
            iconSz={local.iconSz}
            label={local.label}
          />
        )}
      />
      <Portal>
        <Tooltip.Positioner>
          {/* zag lifts the content's z-index onto the positioner. */}
          <Tooltip.Content
            class={
              'text-xs text-text px-2.5 py-1.5 border border-border rounded-md bg-bg-card max-w-xs shadow-lg z-50'
            }
          >
            {local.label}
          </Tooltip.Content>
        </Tooltip.Positioner>
      </Portal>
    </Tooltip.Root>
  );
};
