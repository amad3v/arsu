import { Menu } from '@ark-ui/solid/menu';
import { Show } from 'solid-js';
import { Portal } from 'solid-js/web';

import type { Component, ParentComponent } from 'solid-js';

/**
 * A menu's popup, portalled to <body> so that no scrolling or clipping parent
 * cuts it off. Menus open instantly: they are reached for often, and often
 * from the keyboard.
 */
export const MenuPanel: ParentComponent = (props) => (
  <Portal>
    <Menu.Positioner>
      <Menu.Content class={'menu-content'}>{props.children}</Menu.Content>
    </Menu.Positioner>
  </Portal>
);

export interface MenuRadioOptionProps {
  value: string;
  label: string;
  /** A decorative icon class before the label, e.g. `i-ph-sun`. */
  icon?: string;
}

/** One choice in a Menu.RadioItemGroup; a check marks the current one, not colour alone. */
export const MenuRadioOption: Component<MenuRadioOptionProps> = (props) => (
  <Menu.RadioItem value={props.value} class={'menu-item'}>
    <Show when={props.icon}>
      {(icon) => <i class={`${icon()} text-text-muted size-4`} aria-hidden={'true'} />}
    </Show>
    <Menu.ItemText class={'flex-1'}>{props.label}</Menu.ItemText>
    <Menu.ItemIndicator class={'flex'}>
      <i class={'i-ph-check size-4'} aria-hidden={'true'} />
    </Menu.ItemIndicator>
  </Menu.RadioItem>
);
