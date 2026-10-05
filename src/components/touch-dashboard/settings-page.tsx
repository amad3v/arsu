import { createSignal, Show } from 'solid-js';
import { Portal } from 'solid-js/web';

import { AUTO_LOCK_MINUTES } from '@api/settings';
import { AboutDialog } from '@cpt/about-dialog';
import { APP_NAME } from '@cpt/app-name';
import { createBackDismiss } from '@cpt/back-dismiss';
import { useSettings } from '@cpt/settings';
import { clipboardClearChoices, minutesLabel, secondsLabel } from '@cpt/settings/options';
import { Sheet, SheetChoices } from '@cpt/sheet';
import { themeIcon, themeLabel } from '@cpt/theme-toggle/data';
import { ThemeSheet } from '@cpt/theme-toggle/theme-sheet';

import { BiometricSetting } from './biometric-setting';

import type { Component, JSX } from 'solid-js';

export interface SettingsPageProps {
  open: boolean;
  onClose: () => void;
}

/** Which setting's choices are showing in the sheet. */
type Picking = 'auto-lock' | 'clipboard' | null;

/**
 * The touch interface's settings, as a page of its own: the desktop's
 * settings and theme menus, laid out as grouped rows. (Import and export are
 * things to do, not settings: they are under the + button.)
 * Each setting opens its choices in a bottom sheet. Back closes the page.
 */
export const SettingsPage: Component<SettingsPageProps> = (props) => {
  const settings = useSettings();
  const [picking, setPicking] = createSignal<Picking>(null);
  const [themeOpen, setThemeOpen] = createSignal(false);
  const [aboutOpen, setAboutOpen] = createSignal(false);

  createBackDismiss(
    () => props.open,
    () => props.onClose(),
    true,
  );

  const sheetTitle = () => {
    switch (picking()) {
      case 'auto-lock':
        return 'Lock when idle for';
      case 'clipboard':
        return 'Clear copied codes after';
      case null:
        return '';
    }
  };

  return (
    <Show when={props.open}>
      <Portal>
        <div
          role={'dialog'}
          aria-modal={'true'}
          aria-label={'Settings'}
          class={'modal-page text-text bg-bg-app flex flex-col inset-0 fixed z-30'}
        >
          <header class={'px-2 flex shrink-0 gap-1 h-14 items-center'}>
            <button
              type={'button'}
              class={'btn-ghost rounded-full size-12'}
              aria-label={'Back'}
              onClick={() => props.onClose()}
            >
              <i class={'i-ph-arrow-left size-6'} aria-hidden={'true'} />
            </button>
            <h1 class={'text-lg font-semibold'}>{'Settings'}</h1>
          </header>

          <div class={'px-4 pb-6 flex flex-1 flex-col gap-5 min-h-0 overflow-y-auto'}>
            <Group title={'Appearance'}>
              <Row
                icon={themeIcon[settings.values.theme]}
                label={'Theme'}
                value={themeLabel[settings.values.theme]}
                onSelect={() => setThemeOpen(true)}
              />
            </Group>

            <Group title={'Security'}>
              <BiometricSetting />
              <Row
                icon={'i-ph-lock-simple'}
                label={'Lock when idle for'}
                value={minutesLabel(settings.values.autoLockMinutes)}
                onSelect={() => setPicking('auto-lock')}
              />
              <Row
                icon={'i-ph-timer'}
                label={'Clear copied codes after'}
                value={secondsLabel(settings.values.clipboardClearSeconds)}
                onSelect={() => setPicking('clipboard')}
              />
            </Group>

            <Group>
              <Row
                icon={'i-ph-info'}
                label={`About ${APP_NAME}`}
                onSelect={() => setAboutOpen(true)}
              />
            </Group>
          </div>
        </div>
      </Portal>

      <Sheet open={picking() !== null} title={sheetTitle()} onClose={() => setPicking(null)}>
        <Show when={picking() === 'auto-lock'}>
          <SheetChoices
            label={'Lock when idle for'}
            choices={AUTO_LOCK_MINUTES}
            current={settings.values.autoLockMinutes}
            choiceLabel={minutesLabel}
            onChoose={(autoLockMinutes) => {
              setPicking(null);
              void settings.update({ autoLockMinutes });
            }}
          />
        </Show>
        <Show when={picking() === 'clipboard'}>
          <SheetChoices
            label={'Clear copied codes after'}
            choices={clipboardClearChoices(settings.values.clipboardClearSeconds)}
            current={settings.values.clipboardClearSeconds}
            choiceLabel={secondsLabel}
            onChoose={(clipboardClearSeconds) => {
              setPicking(null);
              void settings.update({ clipboardClearSeconds });
            }}
          />
        </Show>
      </Sheet>

      <ThemeSheet open={themeOpen()} onClose={() => setThemeOpen(false)} />
      <AboutDialog open={aboutOpen()} onClose={() => setAboutOpen(false)} />
    </Show>
  );
};

/** A titled card of rows, as the desktop lists its entries. */
const Group: Component<{ title?: string; children: JSX.Element }> = (props) => (
  <section class={'flex flex-col gap-2'}>
    <Show when={props.title}>
      {(title) => <h2 class={'text-xs text-text-muted font-medium px-1 uppercase'}>{title()}</h2>}
    </Show>
    <div
      class={
        'border border-border rounded-xl bg-bg-card shadow-sm overflow-hidden divide-border divide-y'
      }
    >
      {props.children}
    </div>
  </section>
);

interface RowProps {
  icon: string;
  label: string;
  /** The setting's current value, shown at the end of the row. */
  value?: string;
  onSelect: () => void;
}

const Row: Component<RowProps> = (props) => (
  <button
    type={'button'}
    class={'px-4 text-left pressable flex gap-4 min-h-14 w-full items-center active:bg-bg-pressed'}
    onClick={() => props.onSelect()}
  >
    <i class={`${props.icon} text-text-muted size-5`} aria-hidden={'true'} />
    <span class={'flex-1'}>{props.label}</span>
    <Show when={props.value}>{(value) => <span class={'subtle'}>{value()}</span>}</Show>
    <i class={'i-ph-caret-right text-text-muted size-4'} aria-hidden={'true'} />
  </button>
);
