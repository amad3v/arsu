// The new-password rule the backend enforces for create_vault and
// export_to_aegis_file (crates/cmd/src/password.rs), mirrored so the UI can
// guide the user before submitting. Unlock applies no rule.

/** The minimum length of a new master or export password, in characters. */
export const MIN_PASSWORD_CHARS = 12;

/**
 * The length as the backend counts it: Unicode code points, not UTF-16 code
 * units (`'🔑'.length` is 2, but it is one character).
 */
export function passwordLength(password: string): number {
  // A string's iterator yields code points: the unit Rust's `chars()` counts.
  return Array.from(password).length;
}

export function meetsPasswordPolicy(password: string): boolean {
  return passwordLength(password) >= MIN_PASSWORD_CHARS;
}

/** Inline guidance for a new password typed twice. */
export interface NewPasswordCheck {
  /** What the password still needs, or null once it meets the rule. */
  password: string | null;
  /** Set when a confirmation has been typed and differs; null otherwise. */
  confirmation: string | null;
  /** True when the pair can be submitted. */
  valid: boolean;
}

export function checkNewPassword(password: string, confirmation: string): NewPasswordCheck {
  const missing = MIN_PASSWORD_CHARS - passwordLength(password);
  const matches = confirmation === password;

  return {
    password: missing <= 0 ? null : lengthGuidance(missing),
    confirmation: confirmation === '' || matches ? null : "The passwords don't match.",
    valid: missing <= 0 && matches,
  };
}

function lengthGuidance(missing: number): string {
  const rule = `Use at least ${MIN_PASSWORD_CHARS} characters`;
  if (missing === MIN_PASSWORD_CHARS) return `${rule}.`;
  return `${rule}: ${missing} more to go.`;
}
