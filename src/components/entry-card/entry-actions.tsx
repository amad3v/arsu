import { Menu } from '@ark-ui/solid/menu';

import { MenuPanel } from '@cpt/menu';

import type { Component } from 'solid-js';

export interface EntryActionsProps {
  /** The entry's label ("GitHub (alice)"), to name the menu for screen readers. */
  label: string;
  /** The DOM id for the trigger button, so a row dialog (Delete, Export) can return focus to it on close. */
  triggerId: string;
  onExport: () => void;
  onDelete: () => void;
}

/**
 * A row's rarer actions, kept behind one quiet button so that the code stays
 * the row's primary control and a list of rows isn't a column of red buttons.
 */
export const EntryActions: Component<EntryActionsProps> = (props) => (
  <Menu.Root
    lazyMount
    unmountOnExit
    ids={{ trigger: props.triggerId }}
    positioning={{ placement: 'bottom-end' }}
  >
    <Menu.Trigger
      class={'btn-ghost text-text-muted shrink-0 size-8 hover:text-text'}
      aria-label={`More actions for ${props.label}`}
    >
      <i class={'i-ph-dots-three-vertical-bold size-5'} aria-hidden={'true'} />
    </Menu.Trigger>

    <MenuPanel>
      <Menu.Item value={'export'} class={'menu-item'} onSelect={() => props.onExport()}>
        <i class={'i-ph-qr-code text-text-muted size-4'} aria-hidden={'true'} />
        {'Show QR code…'}
      </Menu.Item>
      <Menu.Separator class={'menu-separator'} />
      <Menu.Item
        value={'delete'}
        class={'menu-item text-error-text'}
        onSelect={() => props.onDelete()}
      >
        <i class={'i-ph-trash size-4'} aria-hidden={'true'} />
        {'Delete…'}
      </Menu.Item>
    </MenuPanel>
  </Menu.Root>
);
