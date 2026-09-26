import { Field } from '@ark-ui/solid/field';

import { FieldMessages } from './field-messages';
import { createFieldText } from './field-text';

import type { FieldTextProps } from './field-text';
import type { Component, Ref } from 'solid-js';

export interface TextFieldProps extends FieldTextProps {
  label: string;
  value: string;
  onValueChange: (value: string) => void;
  onBlur?: () => void;
  placeholder?: string;
  required?: boolean;
  disabled?: boolean;
  /** Monospaced text, for secrets and codes. */
  monospace?: boolean;
  ref?: Ref<HTMLInputElement>;
}

/**
 * A labelled one-line text input with helper and error text. For names,
 * secrets and links: no autocomplete history and no spell checking.
 */
export const TextField: Component<TextFieldProps> = (props) => {
  const text = createFieldText(props);

  // Solid compiles every `ref` given to a component into a callback.
  const setInput = (element: HTMLInputElement) => {
    const ref = props.ref;
    if (typeof ref === 'function') ref(element);
  };

  return (
    <Field.Root
      ids={text.ids}
      invalid={Boolean(props.error)}
      required={props.required}
      disabled={props.disabled}
    >
      <Field.Label class={'label'}>{props.label}</Field.Label>
      <Field.Input
        ref={setInput}
        class={props.monospace ? 'input font-mono' : 'input'}
        value={props.value}
        onInput={(event) => props.onValueChange(event.currentTarget.value)}
        onBlur={() => props.onBlur?.()}
        placeholder={props.placeholder}
        aria-describedby={text.describedBy()}
        autocomplete={'off'}
        spellcheck={false}
      />
      <FieldMessages
        text={text}
        description={props.description}
        error={props.error}
        warning={props.warning}
      />
    </Field.Root>
  );
};
