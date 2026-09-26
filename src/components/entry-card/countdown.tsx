import { createMemo, Show } from 'solid-js';

import { elapsedInStep, isUrgent, secondsLeft } from './code-timing';

import './entry-card.css';

import type { Component } from 'solid-js';

export interface CountdownProps {
  /** When the code expires, in ms since the epoch; null before the first fetch. */
  expiresAt: number | null;
  /** The TOTP period in seconds. */
  period: number | null;
  /** The shared clock, ticking on whole seconds. */
  now: number;
}

/**
 * Seconds left on a TOTP code, over a bar that drains across the code's
 * period. In the last seconds both turn to the warning colour and a timer icon
 * appears, so the warning is not told by colour alone.
 */
export const Countdown: Component<CountdownProps> = (props) => {
  const urgent = () =>
    props.expiresAt !== null && isUrgent(secondsLeft(props.expiresAt, props.now));

  // One bar per code: its CSS animation drains it on the compositor, starting
  // part-way through when the code was fetched mid-step. A new code (a new
  // expiry) mounts a new bar, which starts the animation afresh.
  const bar = createMemo(() => {
    const { expiresAt, period } = props;
    if (expiresAt === null || period === null) return null;
    return { periodMs: period * 1000, elapsedMs: elapsedInStep(expiresAt, period, Date.now()) };
  });

  return (
    <div
      class={
        'text-text-muted flex shrink-0 flex-col gap-1 w-12 items-end data-[urgent]:text-warning'
      }
      data-urgent={urgent() ? '' : undefined}
    >
      <Show when={props.expiresAt}>
        {(expiresAt) => (
          <span class={'text-xs font-medium inline-flex gap-0.5 items-center tabular-nums'}>
            <Show when={urgent()}>
              <i class={'i-ph-timer size-3.5'} aria-hidden={'true'} />
            </Show>
            <span aria-hidden={'true'}>{`${secondsLeft(expiresAt(), props.now)}s`}</span>
            <span class={'sr-only'}>{`${secondsLeft(expiresAt(), props.now)} seconds left`}</span>
          </span>
        )}
      </Show>
      <Show when={bar()} keyed>
        {(timing) => (
          <span class={'countdown-track rounded-full bg-border h-1 w-10 overflow-hidden'}>
            <span
              class={'countdown-bar'}
              style={{
                'animation-duration': `${timing.periodMs}ms`,
                'animation-delay': `-${timing.elapsedMs}ms`,
              }}
            />
          </span>
        )}
      </Show>
    </div>
  );
};
