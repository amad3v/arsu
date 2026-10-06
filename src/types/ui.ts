import type { JSX, Ref } from 'solid-js';

export interface ModalProps {
  open: boolean;
  title: string;
  /** Called when the user dismisses the dialog (close button, Escape, click outside). */
  onClose: () => void;
  /** A visible sentence under the title, announced with it. */
  description?: string;
  wide?: boolean;
  /**
   * The element to focus once the dialog opens, instead of the first
   * tabbable element (which is usually the close button). Read once the
   * dialog's focus trap activates, so it never races a native `[autofocus]`
   * attribute or a field focusing itself on mount.
   */
  initialFocusEl?: () => HTMLElement | null;
  /**
   * The element to return focus to once the dialog closes, instead of
   * whatever was focused right before it opened (which a race with
   * `initialFocusEl` can otherwise get wrong, dropping focus to `<body>`).
   */
  finalFocusEl?: () => HTMLElement | null;
  /**
   * In the touch interface, a full-screen page rather than a bottom sheet:
   * for a dialog that is a place to read in (About), not a question or a
   * short form.
   */
  page?: boolean;
  /** Called once the dialog has closed and its exit animation has played. */
  onExitComplete?: () => void;
}

export interface BaseButtonProps extends Omit<
  JSX.ButtonHTMLAttributes<HTMLButtonElement>,
  'aria-label' | 'children'
> {
  /** The accessible name. Say what the button does: "Lock vault", "Copy code". */
  label: string;
  /** The icon's class, e.g. `i-ph-lock`. The icon is decorative. */
  icon: string;
  /** The icon's size utility. Defaults to `size-5`. */
  iconSz?: string;
}

/** IconifiedButton shows `label` as its tooltip too. */
export type IconifiedButtonProps = BaseButtonProps;

export interface PasswordFieldProps {
  label: string;
  value: string;
  onValueChange: (value: string) => void;
  /** `current-password` for a password the user knows, `new-password` for one being chosen. */
  autocomplete: 'current-password' | 'new-password';
  /** Helper text under the field, e.g. the password rule. */
  description?: string | null;
  /** Marks the field invalid and shows this under it; announced with the field. */
  error?: string | null;
  /** Warn while Caps Lock is on: for passwords typed from memory. */
  capsLockHint?: boolean;
  placeholder?: string;
  required?: boolean;
  disabled?: boolean;
  /** The `<input>`, e.g. to select its text after a wrong password, or to give a Modal an `initialFocusEl`. */
  ref?: Ref<HTMLInputElement>;
}
