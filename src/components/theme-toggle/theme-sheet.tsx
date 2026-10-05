import { useSettings } from '@cpt/settings';
import { THEMES } from '@cpt/settings/theme';
import { Sheet, SheetChoices } from '@cpt/sheet';

import { themeLabel } from './data';

import type { Component } from 'solid-js';

export interface ThemeSheetProps {
  open: boolean;
  onClose: () => void;
}

/**
 * The touch interface's theme choice: a bottom sheet of the three themes,
 * the same wherever the theme is chosen (the settings, the lock screen).
 */
export const ThemeSheet: Component<ThemeSheetProps> = (props) => {
  const settings = useSettings();

  return (
    <Sheet open={props.open} title={'Theme'} onClose={() => props.onClose()}>
      <SheetChoices
        label={'Theme'}
        choices={THEMES}
        current={settings.values.theme}
        choiceLabel={(theme) => themeLabel[theme]}
        onChoose={(theme) => {
          props.onClose();
          void settings.update({ theme });
        }}
      />
    </Sheet>
  );
};
