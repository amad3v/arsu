import { createMemo } from 'solid-js';

import { checkNewPassword, MIN_PASSWORD_CHARS } from '@api/password-policy';
import { PasswordField } from '@cpt/password-field';

import type { Component, Ref } from 'solid-js';

export interface NewPasswordFieldsProps {
  /** "Master password", "Backup password". */
  label: string;
  /** "Confirm master password". */
  confirmLabel: string;
  password: string;
  confirmation: string;
  onPasswordChange: (password: string) => void;
  onConfirmationChange: (confirmation: string) => void;
  /** The backend's rejection of the password (WeakPassword), shown on the first field. */
  error?: string | null;
  /** The first field's `<input>`, e.g. to give a Modal an `initialFocusEl`, or for a screen to focus it on mount. */
  passwordRef?: Ref<HTMLInputElement>;
}

/**
 * A new password typed twice, for the master password and the backup
 * password. Under each field it shows, as the user types, what the password
 * still needs and whether the two match. Submit only when
 * `checkNewPassword(password, confirmation).valid`.
 */
export const NewPasswordFields: Component<NewPasswordFieldsProps> = (props) => {
  const check = createMemo(() => checkNewPassword(props.password, props.confirmation));
  const matches = () => props.confirmation !== '' && props.confirmation === props.password;

  return (
    <>
      <PasswordField
        ref={props.passwordRef}
        label={props.label}
        value={props.password}
        onValueChange={(value) => props.onPasswordChange(value)}
        autocomplete={'new-password'}
        description={check().password ?? `At least ${MIN_PASSWORD_CHARS} characters ✓`}
        error={props.error}
        capsLockHint
        required
      />
      <PasswordField
        label={props.confirmLabel}
        value={props.confirmation}
        onValueChange={(value) => props.onConfirmationChange(value)}
        autocomplete={'new-password'}
        description={matches() ? 'The passwords match ✓' : null}
        error={check().confirmation}
        required
      />
    </>
  );
};
