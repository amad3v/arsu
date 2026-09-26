import { scrollFade } from '@cpt/scroll-fade';
import { ThemeToggle } from '@cpt/theme-toggle';

import type { ParentComponent, Ref } from 'solid-js';

export interface AuthShellProps {
  title: string;
  subtitle: string;
  ref?: Ref<HTMLDivElement>;
}

/** The full-window frame of the create-vault and unlock screens. */
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
    </div>
  );
};
