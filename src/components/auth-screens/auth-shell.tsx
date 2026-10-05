import { Show } from 'solid-js';

import { scrollFade } from '@cpt/scroll-fade';
import { ThemeToggle } from '@cpt/theme-toggle';
import { isTouchUi } from '@cpt/touch-ui';

import appIcon from '../../assets/app-icon.svg';

import type { ParentComponent, Ref } from 'solid-js';

export interface AuthShellProps {
  title: string;
  subtitle: string;
  ref?: Ref<HTMLDivElement>;
}

/** Read once: the touch interface lays these screens out for a phone. */
const TOUCH_UI = isTouchUi();

/**
 * The full-window frame of the create-vault and unlock screens.
 *
 * On the desktop, a card in the middle of the window. In the touch
 * interface, a phone's lock screen: the app's icon and the title in the
 * upper part, the controls in the lower part, within reach of a thumb and
 * just above the keyboard when it opens. The theme is chosen in the
 * settings only: nothing on a lock screen but what unlocks.
 */
export const AuthShell: ParentComponent<AuthShellProps> = (props) => {
  return (
    <div
      ref={(element) => {
        scrollFade(element);
        // Solid compiles every `ref` given to a component into a callback.
        const ref = props.ref;
        if (typeof ref === 'function') ref(element);
      }}
      class={'scroll-fade bg-bg-app inset-0 fixed z-20 overflow-y-auto'}
    >
      <Show
        when={TOUCH_UI}
        fallback={
          <>
            {/* Floating theme toggle */}
            <div class={'right-4 top-4 absolute'}>
              <ThemeToggle />
            </div>

            <div class={'p-4 flex min-h-full items-center justify-center'}>
              <div class={'card max-w-md w-full'}>
                <h1 class={'text-2xl text-text font-semibold'}>{props.title}</h1>
                <p class={'subtle mb-6 mt-1'}>{props.subtitle}</p>
                {props.children}
              </div>
            </div>
          </>
        }
      >
        <div class={'px-6 pb-8 pt-12 flex flex-col gap-8 min-h-full'}>
          <div class={'text-center flex flex-1 flex-col gap-3 items-center justify-center'}>
            <img src={appIcon} alt={''} class={'rounded-2xl size-20 shadow-sm'} />
            <h1 class={'text-2xl text-text font-semibold'}>{props.title}</h1>
            <p class={'subtle text-base max-w-sm'}>{props.subtitle}</p>
          </div>
          <div class={'mx-auto max-w-md w-full'}>{props.children}</div>
        </div>
      </Show>
    </div>
  );
};
