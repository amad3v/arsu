import { Menu } from '@ark-ui/solid/menu';
import { createSignal, For } from 'solid-js';

import { AUTO_LOCK_MINUTES } from '@api/settings';
import { AboutDialog } from '@cpt/about-dialog';
import { APP_NAME } from '@cpt/app-name';
import { MenuPanel, MenuRadioOption } from '@cpt/menu';

import {
  clipboardClearChoices,
  minutesLabel,
  parseAutoLockMinutes,
  parseClipboardClearSeconds,
  secondsLabel,
} from './options';
import { useSettings } from './use-settings';

import type { Component } from 'solid-js';

/**
 * The header's settings menu: one entry per setting, each opening a submenu
 * of its values, then the About dialog.
 */
export const SettingsMenu: Component = () => {
  const settings = useSettings();
  const [aboutOpen, setAboutOpen] = createSignal(false);
  let trigger: HTMLButtonElement | undefined;

  return (
    <>
      <Menu.Root lazyMount unmountOnExit positioning={{ placement: 'bottom-end' }}>
        <Menu.Trigger
          ref={(element) => {
            trigger = element;
          }}
          class={'btn'}
          aria-label={'Settings'}
        >
          <i class={'i-ph-gear-six size-5'} aria-hidden={'true'} />
        </Menu.Trigger>

        <MenuPanel>
          <ChoiceSubmenu
            label={'Lock when idle for'}
            icon={'i-ph-lock-simple'}
            current={settings.values.autoLockMinutes}
            choices={AUTO_LOCK_MINUTES}
            choiceLabel={minutesLabel}
            onChoose={(value) => {
              const minutes = parseAutoLockMinutes(value);
              if (minutes !== null) void settings.update({ autoLockMinutes: minutes });
            }}
          />
          <ChoiceSubmenu
            label={'Clear copied codes after'}
            icon={'i-ph-timer'}
            current={settings.values.clipboardClearSeconds}
            choices={clipboardClearChoices(settings.values.clipboardClearSeconds)}
            choiceLabel={secondsLabel}
            onChoose={(value) => {
              const seconds = parseClipboardClearSeconds(value);
              if (seconds !== null) void settings.update({ clipboardClearSeconds: seconds });
            }}
          />

          <Menu.Separator class={'menu-separator'} />

          <Menu.Item value={'about'} class={'menu-item'} onSelect={() => setAboutOpen(true)}>
            <i class={'i-ph-info text-text-muted size-4'} aria-hidden={'true'} />
            {`About ${APP_NAME}`}
          </Menu.Item>
        </MenuPanel>
      </Menu.Root>

      <AboutDialog
        open={aboutOpen()}
        onClose={() => setAboutOpen(false)}
        finalFocusEl={() => trigger ?? null}
      />
    </>
  );
};

interface ChoiceSubmenuProps {
  label: string;
  /** A decorative icon class before the label, as every entry of the menu has. */
  icon: string;
  current: number;
  choices: readonly number[];
  choiceLabel: (value: number) => string;
  /** The chosen value, as the menu hands it back: a string. */
  onChoose: (value: string) => void;
}

/**
 * A menu entry naming a setting and its current value, opening a submenu of
 * the values to pick from. Each submenu is its own menu, so its items'
 * highlight is its own: the values alone (10, 15, 30…) repeat across settings,
 * and items sharing a value in one menu were highlighted together.
 */
const ChoiceSubmenu: Component<ChoiceSubmenuProps> = (props) => (
  <Menu.Root lazyMount unmountOnExit>
    <Menu.TriggerItem class={'menu-item'}>
      <i class={`${props.icon} text-text-muted size-4`} aria-hidden={'true'} />
      <span class={'flex-1'}>{props.label}</span>
      <span class={'text-xs text-text-muted'}>{props.choiceLabel(props.current)}</span>
      <i class={'i-ph-caret-right text-text-muted size-4'} aria-hidden={'true'} />
    </Menu.TriggerItem>

    <MenuPanel>
      <Menu.RadioItemGroup
        value={String(props.current)}
        onValueChange={(details) => props.onChoose(details.value)}
      >
        <For each={props.choices}>
          {(value) => <MenuRadioOption value={String(value)} label={props.choiceLabel(value)} />}
        </For>
      </Menu.RadioItemGroup>
    </MenuPanel>
  </Menu.Root>
);
