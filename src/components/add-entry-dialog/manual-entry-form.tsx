import { Collapsible, useCollapsible } from '@ark-ui/solid/collapsible';
import { RadioGroup } from '@ark-ui/solid/radio-group';
import { createEffect, createMemo, createSignal, For, Show } from 'solid-js';
import { createStore } from 'solid-js/store';

import { addEntryManual } from '@api';
import { errorMessage } from '@api/lib';
import { NumberField, SelectField, TextField } from '@cpt/form-fields';

import {
  ALGORITHM_OPTIONS,
  codeSettingsSummary,
  DIGITS_OPTIONS,
  EMPTY_MANUAL_ENTRY,
  fieldForError,
  OTP_TYPE_OPTIONS,
  PERIOD_SECONDS,
  secretWarning,
  validateManualEntry,
} from './manual-entry';

import './manual-entry-form.css';

import type { ManualEntryDraft, ManualEntryField } from './manual-entry';
import type { Component } from 'solid-js';

export interface ManualEntryFormProps {
  onAdded: (id: string) => void;
}

interface SubmitError {
  /** The field the backend rejected, or null for the form as a whole. */
  field: ManualEntryField | null;
  message: string;
}

/**
 * The manual tab: type in an account a service shows as a text secret.
 *
 * A field's error shows once the user has left the field or tried to submit,
 * and then follows what they type. An error from the backend shows on the
 * field it is about, or under the form, until the form changes.
 *
 * The code settings are folded under a one-line summary, as nearly every
 * service uses the defaults; they open on a click, or by themselves when one
 * of them has an error to show.
 *
 * A secret that is valid but shorter than services issue is added only on a
 * second submit: the first shows why and turns the button into "Add anyway".
 */
export const ManualEntryForm: Component<ManualEntryFormProps> = (props) => {
  const [form, setForm] = createStore<ManualEntryDraft>({ ...EMPTY_MANUAL_ENTRY });
  const [visited, setVisited] = createStore<Partial<Record<ManualEntryField, boolean>>>({});
  const [attempted, setAttempted] = createSignal(false);
  const [submitError, setSubmitError] = createSignal<SubmitError | null>(null);
  const [busy, setBusy] = createSignal(false);
  // The short secret a submit warned about: submitting it again adds it.
  // Tied to a submit, not to the warning showing, which it also does on blur:
  // the blur of pressing the button would otherwise confirm in the same click.
  const [warnedSecret, setWarnedSecret] = createSignal<string | null>(null);
  const confirming = () => warnedSecret() === form.secret;
  const settings = useCollapsible();
  // Set when an error in the folded code settings opens them: the field can
  // take focus only once the content is shown, which the Collapsible does
  // after measuring it.
  const [focusWhenShown, setFocusWhenShown] = createSignal(false);
  const validation = createMemo(() => validateManualEntry(form));
  let formElement: HTMLFormElement | undefined;
  let settingsContent: HTMLDivElement | undefined;

  function update<K extends keyof ManualEntryDraft>(key: K, value: ManualEntryDraft[K]) {
    setForm(key, value);
    setSubmitError(null);
  }

  function visit(field: ManualEntryField) {
    setVisited(field, true);
  }

  function fieldError(field: ManualEntryField): string | null {
    const rejected = submitError();
    if (rejected?.field === field) return rejected.message;
    if (!attempted() && visited[field] !== true) return null;
    const result = validation();
    return result.valid ? null : (result.errors[field] ?? null);
  }

  /** Shown when the secret's error would be: once the user has left the field or tried to submit. */
  function secretWarningShown(): string | null {
    return attempted() || visited.secret === true ? secretWarning(form.secret) : null;
  }

  /** A backend rejection that is not about one field. */
  function formError(): string | null {
    const rejected = submitError();
    return rejected !== null && rejected.field === null ? rejected.message : null;
  }

  /**
   * Focuses the first invalid field. One in the folded code settings is
   * focused once they have opened and shown it.
   */
  function focusFirstInvalidField() {
    // Controls only: Ark also marks a number field's wrapping group invalid.
    const field = formElement?.querySelector<HTMLElement>(
      ':is(input, textarea, button)[aria-invalid="true"]',
    );
    if (field === null || field === undefined) return;
    if (settingsContent?.contains(field) === true && !settings().visible) {
      setFocusWhenShown(true);
      settings().setOpen(true);
    } else {
      field.focus();
    }
  }

  createEffect(() => {
    if (focusWhenShown() && settings().visible) {
      setFocusWhenShown(false);
      focusFirstInvalidField();
    }
  });

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (busy()) return;
    setAttempted(true);

    const result = validation();
    if (!result.valid) {
      focusFirstInvalidField();
      return;
    }
    if (secretWarning(form.secret) !== null && !confirming()) {
      setWarnedSecret(form.secret);
      return;
    }

    setBusy(true);
    setSubmitError(null);
    try {
      props.onAdded(await addEntryManual(result.input));
    } catch (err) {
      setSubmitError({ field: fieldForError(err), message: errorMessage(err) });
      focusFirstInvalidField();
    } finally {
      setBusy(false);
    }
  }

  return (
    <form
      ref={(element) => {
        formElement = element;
      }}
      class={'flex flex-col gap-4'}
      noValidate
      onSubmit={(event) => {
        void submit(event);
      }}
    >
      <div class={'gap-3 grid md:grid-cols-2'}>
        <TextField
          label={'Issuer (optional)'}
          value={form.issuer}
          onValueChange={(value) => update('issuer', value)}
          onBlur={() => visit('issuer')}
          error={fieldError('issuer')}
          placeholder={'GitHub'}
        />
        <TextField
          label={'Account name'}
          value={form.accountLabel}
          onValueChange={(value) => update('accountLabel', value)}
          onBlur={() => visit('accountLabel')}
          error={fieldError('accountLabel')}
          placeholder={'alice@example.com'}
        />
      </div>

      <TextField
        label={'Secret key'}
        description={"As the service shows it. Spaces and letter case don't matter."}
        value={form.secret}
        onValueChange={(value) => update('secret', value)}
        onBlur={() => visit('secret')}
        error={fieldError('secret')}
        warning={secretWarningShown()}
        placeholder={'JBSW Y3DP EHPK 3PXP'}
        monospace
        required
      />

      <Collapsible.RootProvider value={settings} class={'pt-4 border-t border-border'}>
        <Collapsible.Trigger
          class={'group btn-ghost px-2 py-1.5 w-[calc(100%+1rem)] justify-start -mx-2'}
        >
          <i
            class={
              'i-ph-caret-right text-text-muted size-4 transition-transform group-data-[state=open]:rotate-90'
            }
            aria-hidden={'true'}
          />
          <span class={'text-sm font-medium'}>{'Code settings'}</span>
          <span class={'subtle ml-auto truncate'}>{codeSettingsSummary(form)}</span>
        </Collapsible.Trigger>

        <Collapsible.Content
          ref={(element) => {
            settingsContent = element;
          }}
          class={'code-settings'}
        >
          <div class={'pt-3 space-y-3'}>
            <p class={'subtle'}>
              {'Most services use these defaults. Change them only if the service says so.'}
            </p>

            <RadioGroup.Root
              value={form.type}
              onValueChange={(details) => {
                const chosen = OTP_TYPE_OPTIONS.find((option) => option.value === details.value);
                if (chosen !== undefined) update('type', chosen.value);
              }}
              orientation={'horizontal'}
            >
              <RadioGroup.Label class={'label'}>{'Type'}</RadioGroup.Label>
              <div class={'flex flex-wrap gap-x-5 gap-y-2'}>
                <For each={OTP_TYPE_OPTIONS}>
                  {(option) => (
                    <RadioGroup.Item
                      value={option.value}
                      class={'text-sm text-text inline-flex gap-2 items-center'}
                    >
                      <RadioGroup.ItemControl
                        class={
                          'border border-border rounded-full bg-bg-card size-4 transition-colors data-[state=checked]:(border-4 border-primary) data-[focus-visible]:(ring-2 ring-primary ring-offset-1)'
                        }
                      />
                      <RadioGroup.ItemText>{option.label}</RadioGroup.ItemText>
                      <RadioGroup.ItemHiddenInput />
                    </RadioGroup.Item>
                  )}
                </For>
              </div>
            </RadioGroup.Root>

            <div class={'gap-3 grid sm:grid-cols-3'}>
              <SelectField
                label={'Algorithm'}
                options={ALGORITHM_OPTIONS}
                value={form.algorithm}
                onValueChange={(value) => update('algorithm', value)}
              />
              <SelectField
                label={'Digits'}
                options={DIGITS_OPTIONS}
                value={form.digits}
                onValueChange={(value) => update('digits', value)}
              />
              <Show
                when={form.type === 'totp'}
                fallback={
                  <NumberField
                    label={'Counter'}
                    description={'The next one to use, usually 0.'}
                    value={form.counter}
                    onValueChange={(value) => update('counter', value)}
                    onBlur={() => visit('counter')}
                    error={fieldError('counter')}
                    min={0}
                    max={Number.MAX_SAFE_INTEGER}
                  />
                }
              >
                <NumberField
                  label={'Period (seconds)'}
                  value={form.period}
                  onValueChange={(value) => update('period', value)}
                  onBlur={() => visit('period')}
                  error={fieldError('period')}
                  min={PERIOD_SECONDS.min}
                  max={PERIOD_SECONDS.max}
                />
              </Show>
            </div>
          </div>
        </Collapsible.Content>
      </Collapsible.RootProvider>

      <Show when={formError()}>
        {(message) => (
          <p class={'error'} role={'alert'}>
            {message()}
          </p>
        )}
      </Show>

      {/* At the bottom of the panel, where every tab's action is. */}
      <button class={'btn-primary mt-auto self-start'} disabled={busy()} type={'submit'}>
        {busy() ? 'Adding…' : confirming() ? 'Add anyway' : 'Add entry'}
      </button>
    </form>
  );
};
