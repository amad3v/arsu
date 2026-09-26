import { createListCollection } from '@ark-ui/solid/collection';
import { Select } from '@ark-ui/solid/select';
import { createMemo, For } from 'solid-js';

import type { JSX } from 'solid-js';

export interface SelectOption<T extends string> {
  value: T;
  /** What the list and the trigger show. */
  label: string;
}

export interface SelectFieldProps<T extends string> {
  label: string;
  options: readonly SelectOption<T>[];
  value: T;
  onValueChange: (value: T) => void;
  disabled?: boolean;
}

/** A labelled single-choice dropdown. Every option is valid, so it has no error state. */
export function SelectField<T extends string>(props: SelectFieldProps<T>): JSX.Element {
  const collection = createMemo(() =>
    createListCollection({
      items: [...props.options],
      itemToValue: (option) => option.value,
      itemToString: (option) => option.label,
    }),
  );

  return (
    <Select.Root
      collection={collection()}
      value={[props.value]}
      onValueChange={(details) => {
        if (details.items.length > 0) props.onValueChange(details.items[0].value);
      }}
      disabled={props.disabled}
      // Fixed, so the list is not clipped by a scrolling dialog.
      positioning={{ sameWidth: true, strategy: 'fixed' }}
    >
      <Select.Label class={'label'}>{props.label}</Select.Label>
      <Select.Control>
        <Select.Trigger class={'input text-left flex gap-2 items-center justify-between'}>
          <Select.ValueText />
          <Select.Indicator class={'text-text-muted flex'}>
            <i class={'i-ph-caret-down size-4'} aria-hidden={'true'} />
          </Select.Indicator>
        </Select.Trigger>
      </Select.Control>
      <Select.Positioner>
        <Select.Content class={'card p-1 space-y-1 focus-visible:outline-none'}>
          <For each={collection().items}>
            {(option) => (
              <Select.Item
                item={option}
                class={'menu-item justify-between data-[state=checked]:font-medium'}
              >
                <Select.ItemText>{option.label}</Select.ItemText>
                <Select.ItemIndicator class={'text-primary flex'}>
                  <i class={'i-ph-check size-4'} aria-hidden={'true'} />
                </Select.ItemIndicator>
              </Select.Item>
            )}
          </For>
        </Select.Content>
      </Select.Positioner>
      <Select.HiddenSelect />
    </Select.Root>
  );
}
