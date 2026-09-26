import { createUniqueId } from 'solid-js';

/** The text a form field shows under its control. */
export interface FieldTextProps {
  /** Helper text, e.g. what the field expects. */
  description?: string | null;
  /** Marks the field invalid and shows this under it. */
  error?: string | null;
  /** A value that is allowed but probably wrong: shown under it while there is no error. */
  warning?: string | null;
}

export interface FieldText {
  /** For `Field.Root`'s `ids`, so its helper and error text use these ids. */
  ids: { helperText: string; errorText: string };
  /** The warning text's id (Ark's Field has no part for one). */
  warningText: string;
  /** The control's `aria-describedby`: whichever of the texts are shown. */
  describedBy: () => string | undefined;
}

/**
 * Links a control to its helper and error text. Ark's Field points the control
 * at its error only through `aria-errormessage`, which screen readers
 * announce unevenly, so the error is listed in `aria-describedby` too: a user
 * who moves focus to an invalid field hears what is wrong with it.
 */
export function createFieldText(props: FieldTextProps): FieldText {
  const id = createUniqueId();
  const helperText = `field-${id}-helper`;
  const errorText = `field-${id}-error`;
  const warningText = `field-${id}-warning`;

  return {
    ids: { helperText, errorText },
    warningText,
    describedBy: () => {
      const shown: string[] = [];
      if (props.description) shown.push(helperText);
      if (props.error) shown.push(errorText);
      else if (props.warning) shown.push(warningText);
      return shown.length > 0 ? shown.join(' ') : undefined;
    },
  };
}
