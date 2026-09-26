import type { Component } from 'solid-js';

export interface ErrorPanelProps {
  /** What failed, in the user's terms. */
  title: string;
  /** The error's message, selectable so it can be copied into a report. */
  message: string;
  /** Tries the failed step again. */
  onRetry: () => void;
}

/** A failure the user can act on: what went wrong, why, and a way to try again. */
export const ErrorPanel: Component<ErrorPanelProps> = (props) => (
  <div role={'alert'} class={'card flex flex-col gap-3 max-w-md w-full'}>
    <div class={'flex gap-2 items-center'}>
      <i class={'i-ph-warning-circle text-error-text shrink-0 size-5'} aria-hidden={'true'} />
      <h2 class={'font-semibold'}>{props.title}</h2>
    </div>
    <p class={'subtle select-text break-words'}>{props.message}</p>
    <button type={'button'} class={'btn-primary self-start'} onClick={() => props.onRetry()}>
      {'Try again'}
    </button>
  </div>
);
