import { Show } from 'solid-js';

import { APP_NAME } from '@cpt/app-name';

import appIcon from '../../assets/app-icon.svg';

import type { Component } from 'solid-js';

export interface AppTitleProps {
  /** How many entries there are (or match a search), once they are loaded. */
  summary: string | null;
  /** Classes placing the title in its header. */
  class?: string;
}

/**
 * What heads both interfaces' entry list: the app's logo (the icon the user
 * knows from the home screen or the launcher), its name, and the entry count.
 */
export const AppTitle: Component<AppTitleProps> = (props) => (
  <div class={`flex gap-2.5 min-w-0 items-center ${props.class ?? ''}`}>
    <img src={appIcon} alt={''} class={'shrink-0 size-8'} />
    <div class={'flex gap-2 min-w-0 items-baseline'}>
      <h1 class={'text-lg font-semibold'}>{APP_NAME}</h1>
      <Show when={props.summary}>{(summary) => <p class={'subtle truncate'}>{summary()}</p>}</Show>
    </div>
  </div>
);
