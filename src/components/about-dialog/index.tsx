import { createResource, createSignal, For, Show } from 'solid-js';

import { copyAboutDetails, getAboutInfo, openLink } from '@api';
import { errorMessage } from '@api/lib';
import { APP_NAME } from '@cpt/app-name';
import { Modal } from '@cpt/modal';
import { scrollFade } from '@cpt/scroll-fade';
import { SegmentTabs } from '@cpt/segment-tabs';
import { toaster } from '@cpt/toaster';
import { isTouchUi } from '@cpt/touch-ui';

import licenseText from '../../../LICENSE?raw';
import appIcon from '../../assets/app-icon.svg';

import type { AppLink } from '@app-types/api';
import type { Component } from 'solid-js';

export interface AboutDialogProps {
  open: boolean;
  onClose: () => void;
  /** Where focus goes back to once it closes: the menu it was opened from. */
  finalFocusEl?: () => HTMLElement | null;
}

type AboutTab = 'licence' | 'details' | 'credits';

/** Read once: the touch interface stacks the header, for a phone's width. */
const TOUCH_UI = isTouchUi();

/** What the app is, in one line. */
const TAGLINE = 'Two-factor codes for Linux and Android, kept in an encrypted vault.';

/** The licence's copyright line, as the LICENSE file (the one copy of the text) has it. */
const COPYRIGHT = licenseText.split('\n').find((line) => line.startsWith('Copyright')) ?? '';

const CREDITS: readonly { name: string; role: string }[] = [
  { name: 'Tauri', role: 'the app shell' },
  { name: 'SolidJS and Ark UI', role: 'the interface' },
  { name: 'UnoCSS and Phosphor Icons', role: 'styling and icons' },
  { name: 'RustCrypto', role: 'Argon2id, AES-GCM and HMAC' },
  { name: 'jsQR', role: 'reading QR codes' },
];

/** The About dialog: the app and its version, links to the project, its licence, details for a bug report, and credits. */
export const AboutDialog: Component<AboutDialogProps> = (props) => (
  <Modal
    open={props.open}
    title={`About ${APP_NAME}`}
    onClose={() => props.onClose()}
    finalFocusEl={props.finalFocusEl}
  >
    <AboutContent />
  </Modal>
);

/** Mounts with the dialog, so the details are read afresh each time it opens. */
const AboutContent: Component = () => {
  const [info] = createResource(getAboutInfo);
  const [tab, setTab] = createSignal<AboutTab>('licence');

  async function open(link: AppLink) {
    try {
      await openLink(link);
    } catch (err) {
      toaster.create({
        type: 'error',
        title: "The link couldn't be opened",
        description: errorMessage(err),
      });
    }
  }

  async function copyDetails() {
    try {
      await copyAboutDetails();
      toaster.create({ type: 'success', title: 'Details copied' });
    } catch (err) {
      toaster.create({
        type: 'error',
        title: "The details couldn't be copied",
        description: errorMessage(err),
      });
    }
  }

  return (
    // On a phone it fills the page, so that the licence scrolls in its box and the page doesn't.
    <div class={TOUCH_UI ? 'flex flex-col gap-5 h-full' : 'flex flex-col gap-5'}>
      <Show
        when={TOUCH_UI}
        fallback={
          <div class={'flex gap-4 items-center'}>
            {/* The app's logo, as its icons are rendered from; its name is next to it. */}
            <img src={appIcon} alt={''} class={'shrink-0 size-14'} />
            <div class={'min-w-0'}>
              <p class={'text-lg font-semibold'}>{APP_NAME}</p>
              <p class={'subtle'}>{`Version ${info()?.version ?? '…'}`}</p>
              <p class={'subtle'}>{TAGLINE}</p>
            </div>
          </div>
        }
      >
        {/* On a phone: the logo, then the name and version, then what the app is, read in turn. */}
        <div class={'pt-2 text-center flex flex-col gap-1 items-center'}>
          <img src={appIcon} alt={''} class={'mb-2 size-20'} />
          <p class={'text-2xl font-semibold'}>{APP_NAME}</p>
          <p class={'subtle'}>{`Version ${info()?.version ?? '…'}`}</p>
          <p class={'text-lg text-text mt-2 max-w-xs'}>{TAGLINE}</p>
        </div>
      </Show>

      <div class={'flex flex-wrap gap-2'}>
        <button type={'button'} class={'btn'} onClick={() => void open('website')}>
          <i class={'i-ph-globe size-4'} aria-hidden={'true'} />
          {'Visit website'}
        </button>
        <button type={'button'} class={'btn'} onClick={() => void open('issues')}>
          <i class={'i-ph-bug size-4'} aria-hidden={'true'} />
          {'Report an issue'}
        </button>
      </div>

      <SegmentTabs
        label={'About'}
        value={tab()}
        onValueChange={setTab}
        fill={TOUCH_UI}
        tabs={[
          {
            value: 'licence',
            label: 'Licence',
            content: () => (
              <div class={'flex flex-col gap-2 min-h-0'}>
                <p class={'subtle'}>{`MIT License · ${COPYRIGHT}`}</p>
                {/*
                  Fills the panel, whose height the other tabs set: the text
                  scrolls in it (edges fading, no scrollbar) rather than
                  making the dialog as tall as the licence.
                */}
                <div
                  class={
                    'border border-border rounded-md bg-bg-input flex flex-1 flex-col h-0 min-h-40'
                  }
                >
                  <pre
                    ref={scrollFade}
                    class={
                      'scroll-fade text-xs text-text-muted font-mono p-3 flex-1 select-text whitespace-pre-wrap overflow-y-auto'
                    }
                  >
                    {licenseText}
                  </pre>
                </div>
              </div>
            ),
          },
          {
            value: 'details',
            label: 'Details',
            content: () => (
              <div class={'flex flex-col gap-3'}>
                <Show
                  when={info()}
                  fallback={
                    <p class={'subtle'}>
                      {info.error === undefined ? 'Loading…' : errorMessage(info.error)}
                    </p>
                  }
                >
                  {(about) => (
                    <dl class={'text-sm gap-x-4 gap-y-1.5 grid grid-cols-[auto_1fr]'}>
                      <dt class={'text-text-muted'}>{'Version'}</dt>
                      <dd>{about().version}</dd>
                      <dt class={'text-text-muted'}>{'Tauri'}</dt>
                      <dd>{about().tauriVersion}</dd>
                      <dt class={'text-text-muted'}>{about().webviewName}</dt>
                      <dd>{about().webviewVersion ?? 'unknown'}</dd>
                      <Show when={about().vaultPath}>
                        {(path) => (
                          <>
                            <dt class={'text-text-muted'}>{'Vault'}</dt>
                            <dd class={'text-xs font-mono select-text break-all'}>{path()}</dd>
                          </>
                        )}
                      </Show>
                      <Show when={about().settingsPath}>
                        {(path) => (
                          <>
                            <dt class={'text-text-muted'}>{'Settings'}</dt>
                            <dd class={'text-xs font-mono select-text break-all'}>{path()}</dd>
                          </>
                        )}
                      </Show>
                    </dl>
                  )}
                </Show>
                <button
                  type={'button'}
                  class={'btn mt-auto self-start'}
                  onClick={() => void copyDetails()}
                >
                  <i class={'i-ph-copy size-4'} aria-hidden={'true'} />
                  {'Copy details'}
                </button>
              </div>
            ),
          },
          {
            value: 'credits',
            label: 'Credits',
            content: () => (
              <div class={'flex flex-col gap-2'}>
                <p class={'subtle'}>{`${APP_NAME} is built on open-source software:`}</p>
                <ul class={'text-sm space-y-1'}>
                  <For each={CREDITS}>
                    {(credit) => (
                      <li>
                        <span class={'font-medium'}>{credit.name}</span>
                        <span class={'text-text-muted'}>{`, ${credit.role}`}</span>
                      </li>
                    )}
                  </For>
                </ul>
              </div>
            ),
          },
        ]}
      />
    </div>
  );
};
