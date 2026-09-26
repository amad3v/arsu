import { Field } from '@ark-ui/solid/field';
import { NumberInput } from '@ark-ui/solid/number-input';

import { FieldMessages } from './field-messages';
import { createFieldText } from './field-text';

import type { FieldTextProps } from './field-text';
import type { Component } from 'solid-js';

export interface NumberFieldProps extends FieldTextProps {
  label: string;
  /** As typed. The caller validates it: see parseWholeNumber. */
  value: string;
  onValueChange: (value: string) => void;
  onBlur?: () => void;
  min: number;
  max: number;
  disabled?: boolean;
}

const stepperClass =
  'inline-flex shrink-0 items-center justify-center size-9 rounded-md border border-border bg-bg-card text-text shadow-sm transition-colors hover:bg-bg-hover disabled:(pointer-events-none opacity-50)';

/**
 * A labelled whole-number input with − and + steppers. It never changes what
 * the user typed: a value out of range stays as typed, for the caller to
 * report as an error, instead of being clamped when the field loses focus.
 */
export const NumberField: Component<NumberFieldProps> = (props) => {
  const text = createFieldText(props);

  return (
    <Field.Root ids={text.ids} invalid={Boolean(props.error)} disabled={props.disabled}>
      <NumberInput.Root
        class={'max-w-44'}
        value={props.value}
        onValueChange={(details) => props.onValueChange(details.value)}
        min={props.min}
        max={props.max}
        step={1}
        clampValueOnBlur={false}
        formatOptions={{ useGrouping: false }}
      >
        <NumberInput.Label class={'label'}>{props.label}</NumberInput.Label>
        <NumberInput.Control class={'flex gap-1 items-center'}>
          <NumberInput.DecrementTrigger class={stepperClass}>
            <i class={'i-ph-minus size-4'} aria-hidden={'true'} />
          </NumberInput.DecrementTrigger>
          <NumberInput.Input
            class={'input text-center tabular-nums'}
            aria-describedby={text.describedBy()}
            onBlur={() => props.onBlur?.()}
          />
          <NumberInput.IncrementTrigger class={stepperClass}>
            <i class={'i-ph-plus size-4'} aria-hidden={'true'} />
          </NumberInput.IncrementTrigger>
        </NumberInput.Control>
      </NumberInput.Root>
      <FieldMessages text={text} description={props.description} error={props.error} />
    </Field.Root>
  );
};
