import { Match, Show, Switch } from 'solid-js';

import { entryLabel } from '@cpt/entry-label';

import { codePlaceholder, groupDigits } from './code-format';
import { liveCode, upcomingCode } from './code-timing';
import { Countdown } from './countdown';
import { EntryActions } from './entry-actions';
import { entryActionsId, entryRowId, entrySubtitle, entryTitle } from './entry-names';

import './entry-card.css';

import type { EntryCode } from './create-entry-code';
import type { Accessor, Component } from 'solid-js';

export { liveCode } from './code-timing';
export { createEntryCode } from './create-entry-code';
export type { EntryCode, EntryCodeOptions } from './create-entry-code';
export { entryActionsId, entryRowId, entrySubtitle, entryTitle } from './entry-names';

/** How long a just-added row stays marked as new; its tint fades out over the end of it. */
export const NEW_ENTRY_MS = 2_400;

export interface EntryCardProps {
  code: EntryCode;
  /** The shared clock, ticking on whole seconds. */
  now: Accessor<number>;
  /** The selected row (by the arrow keys or a click): Enter copies its code. */
  highlighted: boolean;
  /** Its code was copied a moment ago. */
  copied: boolean;
  /** Just added or imported. */
  isNew: boolean;
  /** A click on the row outside its buttons: select it. */
  onSelect: () => void;
  /** Its code button, or a double-click on the row outside its buttons. */
  onCopy: () => void;
  onExport: () => void;
  onDelete: () => void;
}

/**
 * One entry as a compact list row: its name, its code (click to copy), the
 * time left on a TOTP code or a Generate button for HOTP, and a menu with the
 * rarer actions. Like a native list, a click on the row selects it and a
 * double-click copies its code.
 */
export const EntryCard: Component<EntryCardProps> = (props) => {
  const entry = () => props.code.entry;
  const title = () => entryTitle(entry());
  // The accessible names say which account, as rows at one service look alike.
  const label = () => entryLabel(entry());
  const code = () => liveCode(props.code.state, props.now());
  const upcoming = () => upcomingCode(props.code.state, props.now());
  const isHotp = () => entry().otpType === 'hotp';

  const shownCode = () => {
    const current = code();
    return current === null ? codePlaceholder(entry().digits) : groupDigits(current);
  };
  // Each accessible name starts with the visible text, so a voice-control user
  // can say what they see, and adds the entry, since rows look alike.
  const copyLabel = () => {
    const current = code();
    return current === null
      ? `No code yet for ${label()}`
      : `Copy the code for ${label()}: ${groupDigits(current)}`;
  };
  const generateText = () => (props.code.state.busy ? 'Generating…' : 'Generate code');
  // HOTP starts with no code: the row's one control asks for it. Once a code
  // exists (HOTP after a generate, or TOTP always) that same control copies it.
  const needsGenerate = () => isHotp() && code() === null;
  // Whether the row's main control can act right now: not while a generate is
  // in flight, and not on a copy with nothing to copy yet. Marked with
  // aria-disabled rather than the `disabled` attribute, and guarded in the
  // handler, so the control stays one stable, focusable button per row instead
  // of dropping keyboard focus to <body> on every TOTP rollover or HOTP
  // generate.
  const codeUnavailable = () => (needsGenerate() ? props.code.state.busy : code() === null);
  const mainButtonLabel = () =>
    needsGenerate() ? `${generateText()} for ${label()}` : copyLabel();

  function activateMainButton() {
    if (needsGenerate()) {
      if (props.code.state.busy) return;
      void props.code.generate();
    } else {
      if (code() === null) return;
      props.onCopy();
    }
  }

  // Not on the mousedown's default action: it would take focus from the search
  // field (dropping the selection, which shows only while it has focus) and
  // start a text selection that a double-click would then extend over a word.
  function onRowMouseDown(event: MouseEvent) {
    if (event.button !== 0 || !isRowBackground(event)) return;
    event.preventDefault();
    props.onSelect();
  }

  function onRowDoubleClick(event: MouseEvent) {
    if (isRowBackground(event)) props.onCopy();
  }

  return (
    <li
      id={entryRowId(entry().id)}
      role={'row'}
      class={
        'entry-row px-3 py-2 flex gap-3 items-center data-[highlighted]:bg-bg-selected max-sm:gap-1'
      }
      data-highlighted={props.highlighted ? '' : undefined}
      data-new={props.isNew ? '' : undefined}
      aria-current={props.highlighted ? 'true' : undefined}
      style={{ '--entry-new-duration': `${NEW_ENTRY_MS}ms` }}
      onMouseDown={onRowMouseDown}
      onDblClick={onRowDoubleClick}
    >
      <div role={'gridcell'} class={'flex-1 min-w-0'}>
        <p class={'text-sm font-medium flex gap-2 items-center'}>
          <span class={'truncate'}>{title()}</span>
          <Show when={props.isNew}>
            <span
              data-new-badge
              class={
                'text-xs text-success-text px-1.5 border border-success-border rounded-full bg-success-bg shrink-0'
              }
            >
              {'New'}
            </span>
          </Show>
        </p>
        <Show
          when={props.code.state.error}
          fallback={
            <Show when={entrySubtitle(entry())}>
              {(subtitle) => <p class={'text-xs text-text-muted truncate'}>{subtitle()}</p>}
            </Show>
          }
        >
          {(error) => (
            <p class={'text-xs text-error-text flex gap-1 items-center'}>
              <i class={'i-ph-warning-circle shrink-0 size-3.5'} aria-hidden={'true'} />
              <span class={'truncate'} title={error()}>
                {error()}
              </span>
            </p>
          )}
        </Show>
      </div>

      {/*
        Hidden on a phone's width, which has no room for it beside the names:
        the copy button's own check mark still tells a copy apart there.
      */}
      <p
        role={'gridcell'}
        class={'text-xs flex shrink-0 w-28 items-center justify-end max-sm:hidden'}
      >
        <Switch>
          <Match when={props.copied}>
            <span class={'text-success-text inline-flex gap-1 items-center'}>
              <i class={'i-ph-check size-3.5'} aria-hidden={'true'} />
              {'Copied'}
            </span>
          </Match>
          <Match when={upcoming()}>
            {(next) => (
              <span class={'text-text-muted'}>
                {'Next '}
                <span class={'font-mono tabular-nums'}>{groupDigits(next())}</span>
              </span>
            )}
          </Match>
          <Match when={props.highlighted}>
            <span class={'text-text-muted inline-flex gap-1 items-center'} aria-hidden={'true'}>
              <kbd class={'kbd'}>{'Enter'}</kbd>
              {'copies'}
            </span>
          </Match>
        </Switch>
      </p>

      <div role={'gridcell'} class={'flex shrink-0 w-38 justify-end max-sm:w-auto'}>
        {/*
          One stable, always-focusable <button>: its class, content and label
          switch between "ask for a code" and "copy the code" as the state
          changes, instead of being replaced by a different element (which
          used to drop keyboard focus to <body> on every HOTP generate).
        */}
        <button
          type={'button'}
          class={
            needsGenerate()
              ? 'btn text-xs px-2.5 py-1.5'
              : 'text-xl btn-ghost tracking-wide font-mono px-2 py-1 tabular-nums'
          }
          aria-disabled={codeUnavailable() ? 'true' : undefined}
          aria-label={mainButtonLabel()}
          onClick={activateMainButton}
        >
          <Show when={!needsGenerate()} fallback={generateText()}>
            <span classList={{ 'text-text-muted': code() === null }}>{shownCode()}</span>
            <i
              class={`size-4 ${props.copied ? 'i-ph-check text-success-text' : 'i-ph-copy text-text-muted'}`}
              aria-hidden={'true'}
            />
          </Show>
        </button>
      </div>

      <div role={'gridcell'}>
        <Show
          when={isHotp()}
          fallback={
            <Countdown
              expiresAt={props.code.state.expiresAt}
              period={entry().period}
              now={props.now()}
            />
          }
        >
          <div class={'flex shrink-0 w-12 justify-end'}>
            <Show when={code()}>
              <button
                type={'button'}
                class={'btn-ghost text-text-muted size-8 hover:text-text'}
                aria-disabled={props.code.state.busy ? 'true' : undefined}
                onClick={() => {
                  if (props.code.state.busy) return;
                  void props.code.generate();
                }}
                aria-label={`Generate the next code for ${label()}`}
              >
                <i class={'i-ph-arrow-clockwise size-4'} aria-hidden={'true'} />
              </button>
            </Show>
          </div>
        </Show>
      </div>

      <div role={'gridcell'}>
        <EntryActions
          label={label()}
          triggerId={entryActionsId(entry().id)}
          onExport={() => props.onExport()}
          onDelete={() => props.onDelete()}
        />
      </div>
    </li>
  );
};

/** The pointer is on the row itself, not on one of its buttons, which handle their own clicks. */
function isRowBackground(event: MouseEvent): boolean {
  return event.target instanceof Element && event.target.closest('button') === null;
}
