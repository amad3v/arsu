import { Match, Show, Switch } from 'solid-js';

import { codePlaceholder, groupDigits } from '@cpt/entry-card/code-format';
import { liveCode } from '@cpt/entry-card/code-timing';
import { Countdown } from '@cpt/entry-card/countdown';
import { entrySubtitle, entryTitle } from '@cpt/entry-card/entry-names';
import { entryLabel } from '@cpt/entry-label';

import '@cpt/entry-card/entry-card.css';

import type { EntryCode } from '@cpt/entry-card';
import type { Accessor, Component } from 'solid-js';

export interface TouchEntryRowProps {
  /** The row's DOM id, to scroll a new entry into view. */
  id: string;
  code: EntryCode;
  /** The shared clock, ticking on whole seconds. */
  now: Accessor<number>;
  /** Its code was copied a moment ago. */
  copied: boolean;
  /** Just added or imported. */
  isNew: boolean;
  /** A tap on the row: copy its code (generating one first for HOTP). */
  onCopy: () => void;
  /** Its ⋮ button: the entry's other actions. */
  onActions: () => void;
}

/**
 * One entry in the touch interface: the whole row is one large target that
 * copies its code, which is shown big enough to read across a room, and a ⋮
 * button beside it opens the rarer actions. The desktop row's look, sized for
 * a finger.
 */
export const TouchEntryRow: Component<TouchEntryRowProps> = (props) => {
  const entry = () => props.code.entry;
  const label = () => entryLabel(entry());
  const code = () => liveCode(props.code.state, props.now());
  const isHotp = () => entry().otpType === 'hotp';
  const shownCode = () => {
    const current = code();
    return current === null ? codePlaceholder(entry().digits) : groupDigits(current);
  };
  const hotpHint = () => {
    if (props.code.state.busy) return 'Generating…';
    return code() === null ? 'Tap for code' : 'Tap to copy';
  };
  const rowLabel = () => {
    const current = code();
    if (current !== null) return `Copy the code for ${label()}: ${groupDigits(current)}`;
    return isHotp() ? `Generate and copy a code for ${label()}` : `No code yet for ${label()}`;
  };

  return (
    <li
      id={props.id}
      class={'entry-row flex transition-colors items-stretch data-[copied]:bg-success-bg'}
      data-copied={props.copied ? '' : undefined}
      data-new={props.isNew ? '' : undefined}
      style={{ '--entry-new-duration': '2400ms' }}
    >
      <button
        type={'button'}
        class={
          'py-4 pl-4 pr-2 text-left pressable flex flex-1 gap-3 min-w-0 items-center active:bg-bg-pressed'
        }
        aria-label={rowLabel()}
        aria-disabled={props.code.state.busy ? 'true' : undefined}
        onClick={() => {
          if (!props.code.state.busy) props.onCopy();
        }}
      >
        <span class={'flex flex-1 flex-col min-w-0'}>
          <span class={'text-lg font-semibold truncate'}>{entryTitle(entry())}</span>
          <Show
            when={props.code.state.error}
            fallback={
              <span class={'text-sm text-text-muted min-h-5 truncate'}>
                {entrySubtitle(entry())}
              </span>
            }
          >
            {(error) => <span class={'text-sm text-error-text truncate'}>{error()}</span>}
          </Show>
          <span
            class={
              'text-4xl tracking-wider font-medium font-mono mt-1.5 whitespace-nowrap tabular-nums'
            }
            // Seven or eight digits still fit a phone's width on one line.
            classList={{ 'text-text-muted': code() === null, 'text-3xl': entry().digits > 6 }}
          >
            {shownCode()}
          </span>
        </span>

        <span class={'flex shrink-0 w-24 items-center justify-end'}>
          <Switch>
            <Match when={props.copied}>
              <span
                class={'text-base text-success-text font-medium inline-flex gap-1 items-center'}
              >
                <i class={'i-ph-check-bold size-5'} aria-hidden={'true'} />
                {'Copied'}
              </span>
            </Match>
            <Match when={isHotp()}>
              <span class={'text-sm text-text-muted whitespace-nowrap'}>{hotpHint()}</span>
            </Match>
            <Match when={!isHotp()}>
              <Countdown
                expiresAt={props.code.state.expiresAt}
                period={entry().period}
                now={props.now()}
                large
              />
            </Match>
          </Switch>
        </span>
      </button>

      <button
        type={'button'}
        class={'btn-ghost text-text-muted shrink-0 w-12 active:bg-bg-pressed'}
        aria-label={`More actions for ${label()}`}
        onClick={() => props.onActions()}
      >
        <i class={'i-ph-dots-three-vertical-bold size-6'} aria-hidden={'true'} />
      </button>
    </li>
  );
};
