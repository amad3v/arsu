import { Menu } from '@ark-ui/solid/menu';
import { For } from 'solid-js';

import { MenuPanel, MenuRadioOption } from '@cpt/menu';
import { useSettings } from '@cpt/settings';
import { isTheme, THEMES } from '@cpt/settings/theme';

import { themeIcon, themeLabel } from './data';

import type { Component } from 'solid-js';

/** The theme menu: light, dark, or following the OS. Every screen shares the one theme. */
export const ThemeToggle: Component = () => {
  const settings = useSettings();

  return (
    <Menu.Root lazyMount unmountOnExit positioning={{ placement: 'bottom-end' }}>
      <Menu.Trigger class={'btn'} aria-label={`Theme: ${themeLabel[settings.values.theme]}`}>
        <i class={`${themeIcon[settings.values.theme]} size-5`} aria-hidden={'true'} />
      </Menu.Trigger>

      <MenuPanel>
        <Menu.RadioItemGroup
          value={settings.values.theme}
          onValueChange={(details) => {
            if (isTheme(details.value)) void settings.update({ theme: details.value });
          }}
        >
          <Menu.ItemGroupLabel class={'menu-group-label'}>{'Theme'}</Menu.ItemGroupLabel>
          <For each={THEMES}>
            {(theme) => (
              <MenuRadioOption value={theme} label={themeLabel[theme]} icon={themeIcon[theme]} />
            )}
          </For>
        </Menu.RadioItemGroup>
      </MenuPanel>
    </Menu.Root>
  );
};
