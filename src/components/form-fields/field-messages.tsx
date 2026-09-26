import { Field } from '@ark-ui/solid/field';
import { Show } from 'solid-js';

import type { FieldText, FieldTextProps } from './field-text';
import type { Component } from 'solid-js';

export interface FieldMessagesProps extends FieldTextProps {
  /** From createFieldText: the ids the messages go under. */
  text: FieldText;
}

/**
 * A field's helper text and, while it is invalid, its error, or else its
 * warning. Goes inside `Field.Root`, whose `ids` come from createFieldText.
 * The error and warning are polite live regions, so they are announced as
 * they appear.
 */
export const FieldMessages: Component<FieldMessagesProps> = (props) => (
  <>
    <Show when={props.description}>
      {(description) => (
        <Field.HelperText class={'subtle mt-1 block'}>{description()}</Field.HelperText>
      )}
    </Show>
    <Field.ErrorText class={'text-sm text-error-text mt-1 block'}>{props.error}</Field.ErrorText>
    <div id={props.text.warningText} aria-live={'polite'}>
      <Show when={!props.error && props.warning}>
        {(warning) => (
          <p class={'text-sm text-warning mt-1 flex gap-1.5 items-start'}>
            <i class={'i-ph-warning mt-0.5 shrink-0 size-4'} aria-hidden={'true'} />
            {warning()}
          </p>
        )}
      </Show>
    </div>
  </>
);
