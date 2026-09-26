import { Field } from '@ark-ui/solid/field';
import { createSignal } from 'solid-js';

import { addEntryFromUri } from '@api';
import { errorMessage } from '@api/lib';
import { createFieldText, FieldMessages } from '@cpt/form-fields';

import type { Component, Ref } from 'solid-js';

export interface FromLinkProps {
  /** The link as typed or pasted. Kept by the dialog, which also fills it from a paste. */
  link: string;
  onLinkChange: (link: string) => void;
  onAdded: (id: string) => void;
  ref?: Ref<HTMLTextAreaElement>;
}

const DESCRIPTION = 'Some services show this link next to the QR code.';

/** The link tab: add an account from its otpauth:// link. */
export const FromLink: Component<FromLinkProps> = (props) => {
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const text = createFieldText({
    description: DESCRIPTION,
    get error() {
      return error();
    },
  });
  let field: HTMLTextAreaElement | undefined;

  const setField = (element: HTMLTextAreaElement) => {
    field = element;
    // Solid compiles every `ref` given to a component into a callback.
    const ref = props.ref;
    if (typeof ref === 'function') ref(element);
  };

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    const link = props.link.trim();
    if (link === '' || busy()) return;

    setBusy(true);
    setError(null);
    try {
      props.onAdded(await addEntryFromUri(link));
    } catch (err) {
      setError(errorMessage(err));
      field?.focus();
    } finally {
      setBusy(false);
    }
  }

  return (
    <form
      class={'flex flex-col gap-3'}
      noValidate
      onSubmit={(event) => {
        void submit(event);
      }}
    >
      {/* Grows with the panel; the text box takes the room, and gives it back to an error. */}
      <Field.Root ids={text.ids} invalid={error() !== null} class={'flex flex-1 flex-col'}>
        <Field.Label class={'label'}>{'otpauth:// link'}</Field.Label>
        <Field.Textarea
          ref={setField}
          // The dialog's first field: it takes the initial focus, so a pasted
          // link or screenshot works as soon as the dialog opens.
          autofocus
          class={'input text-xs font-mono flex-1 min-h-16 resize-none'}
          value={props.link}
          onInput={(event) => {
            props.onLinkChange(event.currentTarget.value);
            setError(null);
          }}
          placeholder={'otpauth://totp/Example:alice@example.com?secret=…'}
          aria-describedby={text.describedBy()}
          autocomplete={'off'}
          spellcheck={false}
        />
        <FieldMessages text={text} description={DESCRIPTION} error={error()} />
      </Field.Root>

      <button
        class={'btn-primary self-start'}
        disabled={busy() || props.link.trim() === ''}
        type={'submit'}
      >
        {busy() ? 'Adding…' : 'Add from link'}
      </button>
    </form>
  );
};
