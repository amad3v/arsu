import { Field } from '@ark-ui/solid/field';
import { PasswordInput } from '@ark-ui/solid/password-input';
import { createSignal, createUniqueId, Show } from 'solid-js';

import type { PasswordFieldProps } from '@app-types/ui';
import type { Component } from 'solid-js';

/**
 * The app's one password input. Visibility is Ark's own state: the trigger's
 * label ("Show password" / "Hide password"), its icon and the input's type all
 * read it, so they cannot disagree.
 *
 * It never focuses itself: a field inside a dialog gets the initial focus
 * through the Modal's `initialFocusEl`, given its `ref`, and a field on its
 * own screen (UnlockScreen, CreateVaultScreen) focuses its `ref` on mount.
 * Doing it here too raced Ark's own focus-trap activation and dropped
 * keyboard focus to `<body>` on close.
 */
export const PasswordField: Component<PasswordFieldProps> = (props) => {
  const [capsLock, setCapsLock] = createSignal(false);

  // Ark's PasswordInput takes only the helper text from Field, not the error
  // text, so the input's description is wired here, from ids this field owns.
  const id = createUniqueId();
  const helperTextId = `password-field-${id}-helper`;
  const errorTextId = `password-field-${id}-error`;
  const describedBy = () => {
    const ids: string[] = [];
    if (props.description) ids.push(helperTextId);
    if (props.error) ids.push(errorTextId);
    return ids.length > 0 ? ids.join(' ') : undefined;
  };

  const setInput = (element: HTMLInputElement) => {
    // Solid compiles every `ref` given to a component into a callback.
    const ref = props.ref;
    if (typeof ref === 'function') ref(element);
  };

  // keyup as well as keydown: pressing Caps Lock itself reports the new state
  // on keyup in some engines.
  const trackCapsLock = (event: KeyboardEvent) => {
    if (props.capsLockHint) setCapsLock(event.getModifierState('CapsLock'));
  };

  return (
    <Field.Root
      ids={{ helperText: helperTextId, errorText: errorTextId }}
      invalid={Boolean(props.error)}
      required={props.required}
      disabled={props.disabled}
    >
      <Field.Label class={'label'}>{props.label}</Field.Label>

      <PasswordInput.Root autoComplete={props.autocomplete}>
        <PasswordInput.Control class={'relative'}>
          <PasswordInput.Input
            ref={setInput}
            aria-describedby={describedBy()}
            class={'input pr-10'}
            value={props.value}
            placeholder={props.placeholder}
            onInput={(event) => props.onValueChange(event.currentTarget.value)}
            onKeyDown={trackCapsLock}
            onKeyUp={trackCapsLock}
            onBlur={() => setCapsLock(false)}
          />
          <PasswordInput.VisibilityTrigger
            class={
              'text-text-muted p-1.5 rounded-md flex transition-colors items-center right-1.5 top-1/2 absolute hover:text-text hover:bg-bg-app -translate-y-1/2'
            }
          >
            <PasswordInput.Indicator fallback={<i class={'i-ph-eye size-4'} />}>
              <i class={'i-ph-eye-slash size-4'} />
            </PasswordInput.Indicator>
          </PasswordInput.VisibilityTrigger>
        </PasswordInput.Control>
      </PasswordInput.Root>

      <Show when={props.description}>
        {(description) => (
          <Field.HelperText class={'subtle mt-1'}>{description()}</Field.HelperText>
        )}
      </Show>

      {/* Always present, so screen readers announce the hint when it appears. */}
      <div aria-live={'polite'}>
        <Show when={capsLock()}>
          <p class={'text-sm text-text-muted mt-1 flex gap-1.5 items-center'}>
            <i class={'i-ph-arrow-fat-line-up size-4'} aria-hidden={'true'} />
            {'Caps Lock is on.'}
          </p>
        </Show>
      </div>

      <Field.ErrorText class={'text-sm text-error-text mt-1'}>{props.error}</Field.ErrorText>
    </Field.Root>
  );
};
