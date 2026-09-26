import { Show } from 'solid-js';

import type { Component, Ref } from 'solid-js';

export interface SearchFieldProps {
  ref: Ref<HTMLInputElement>;
  value: string;
  onValueChange: (value: string) => void;
  /** The clear button: empty the field and keep it focused. */
  onClear: () => void;
  /** Keys pressed in the field: the list's arrow-key and Enter handling. */
  onKeyDown: (event: KeyboardEvent) => void;
  onFocusChange: (focused: boolean) => void;
  /** The id of the list the field filters. */
  controls: string;
  /** The DOM id of the row the arrow keys are on, for aria-activedescendant. Null when the list is empty. */
  activeDescendant: string | null;
}

const HINT_ID = 'entry-search-hint';

/** The dashboard's search: filters the list, and owns its keyboard path. */
export const SearchField: Component<SearchFieldProps> = (props) => (
  <div role={'search'} class={'relative'}>
    <label for={'entry-search'} class={'sr-only'}>
      {'Search entries'}
    </label>
    <i
      class={
        'i-ph-magnifying-glass text-text-muted size-4 pointer-events-none left-3 top-1/2 absolute -translate-y-1/2'
      }
      aria-hidden={'true'}
    />
    <input
      ref={props.ref}
      id={'entry-search'}
      // Not type="search": WebKit's own clear button would sit inside the
      // text, beside the slot the hint and the clear button below share.
      type={'text'}
      class={'peer input pl-9 pr-10'}
      placeholder={'Search by issuer or account'}
      autocomplete={'off'}
      spellcheck={false}
      // ARIA 1.2 combobox: the list below is always its popup (it never
      // collapses), and aria-activedescendant tells assistive tech which row
      // the arrow keys are on, since focus itself stays in this field.
      role={'combobox'}
      aria-expanded={'true'}
      aria-controls={props.controls}
      aria-activedescendant={props.activeDescendant ?? undefined}
      aria-describedby={HINT_ID}
      value={props.value}
      onInput={(event) => props.onValueChange(event.currentTarget.value)}
      onKeyDown={(event) => props.onKeyDown(event)}
      onFocus={() => props.onFocusChange(true)}
      onBlur={() => props.onFocusChange(false)}
    />
    {/*
      One slot at the field's end: the clear button while there is text,
      otherwise the "/" hint, which is only shown while focus is elsewhere
      (it says how to get here).
    */}
    <Show
      when={props.value !== ''}
      fallback={
        <kbd
          class={
            'kbd pointer-events-none right-3 top-1/2 absolute peer-focus:hidden -translate-y-1/2'
          }
          aria-hidden={'true'}
        >
          {'/'}
        </kbd>
      }
    >
      <button
        type={'button'}
        class={
          'btn-ghost text-text-muted size-6 right-2 top-1/2 absolute hover:text-text -translate-y-1/2'
        }
        aria-label={'Clear search'}
        // Escape does the same from the field, so the button is left out of
        // the tab order; and a click on it doesn't take focus from the field.
        tabIndex={-1}
        onMouseDown={(event) => event.preventDefault()}
        onClick={() => props.onClear()}
      >
        <i class={'i-ph-x size-4'} aria-hidden={'true'} />
      </button>
    </Show>
    <p id={HINT_ID} class={'sr-only'}>
      {
        'The up and down arrow keys choose an entry, Enter copies its code, and Escape clears the search.'
      }
    </p>
  </div>
);
