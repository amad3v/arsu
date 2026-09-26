// The manual-entry form's rules, mirrored from the backend
// (qr::ParsedAccount::new and the otp crate) so mistakes show next to their
// field before anything is sent. The backend stays the authority: its errors
// are mapped back onto the same fields by fieldForError.

import { isAppErrorPayload } from '@api/lib';
import { parseWholeNumber } from '@cpt/form-fields';

import type {
  Algorithm,
  AppErrorKind,
  HotpInput,
  ManualEntryInput,
  OtpType,
  TotpInput,
} from '@app-types/api';
import type { SelectOption } from '@cpt/form-fields';

export type DigitsChoice = '6' | '7' | '8';

/** The form as typed. Numbers stay text until they are validated. */
export interface ManualEntryDraft {
  issuer: string;
  accountLabel: string;
  secret: string;
  algorithm: Algorithm;
  digits: DigitsChoice;
  type: OtpType;
  period: string;
  counter: string;
}

/** An empty form, set to the Key URI Format defaults that most services use. */
export const EMPTY_MANUAL_ENTRY: Readonly<ManualEntryDraft> = {
  issuer: '',
  accountLabel: '',
  secret: '',
  algorithm: 'sha1',
  digits: '6',
  type: 'totp',
  period: '30',
  counter: '0',
};

export const ALGORITHM_OPTIONS: readonly SelectOption<Algorithm>[] = [
  { value: 'sha1', label: 'SHA-1' },
  { value: 'sha256', label: 'SHA-256' },
  { value: 'sha512', label: 'SHA-512' },
];

export const DIGITS_OPTIONS: readonly SelectOption<DigitsChoice>[] = [
  { value: '6', label: '6' },
  { value: '7', label: '7' },
  { value: '8', label: '8' },
];

export const OTP_TYPE_OPTIONS: readonly SelectOption<OtpType>[] = [
  { value: 'totp', label: 'Time-based (TOTP)' },
  { value: 'hotp', label: 'Counter-based (HOTP)' },
];

/** The TOTP periods the backend accepts, in seconds. */
export const PERIOD_SECONDS = { min: 1, max: 300 } as const;

/** The fields that can show an error. */
export type ManualEntryField = 'issuer' | 'accountLabel' | 'secret' | 'period' | 'counter';

/**
 * The code settings in one line, for their folded header: "TOTP · SHA-1 ·
 * 6 digits · 30 s", or "HOTP · SHA-1 · 6 digits · counter 0". Numbers are
 * shown as typed.
 */
export function codeSettingsSummary(form: ManualEntryDraft): string {
  const algorithm = ALGORITHM_OPTIONS.find((option) => option.value === form.algorithm)?.label;
  const last = form.type === 'totp' ? `${form.period} s` : `counter ${form.counter}`;
  return [form.type.toUpperCase(), algorithm, `${form.digits} digits`, last].join(' · ');
}

export type ManualEntryErrors = Partial<Record<ManualEntryField, string>>;

export type ManualEntryValidation =
  | { valid: true; input: ManualEntryInput }
  | { valid: false; errors: ManualEntryErrors };

const COLON_ERROR = "Remove the colon (:). A name can't contain one.";

/**
 * Checks the form and, if it is valid, builds what add_entry_manual takes.
 * Nothing is replaced by a default: an invalid value is an error on its field.
 */
export function validateManualEntry(form: ManualEntryDraft): ManualEntryValidation {
  const errors: ManualEntryErrors = {};
  const issuer = form.issuer.trim();
  const accountLabel = form.accountLabel.trim();

  if (issuer.includes(':')) errors.issuer = COLON_ERROR;
  if (accountLabel.includes(':')) {
    errors.accountLabel = COLON_ERROR;
  } else if (accountLabel === '' && issuer === '') {
    errors.accountLabel = 'Enter an account name, or at least an issuer.';
  }

  const secretError = checkSecret(form.secret);
  if (secretError !== null) errors.secret = secretError;

  const scheme = readScheme(form);
  if ('error' in scheme) errors[scheme.field] = scheme.error;

  if ('error' in scheme || Object.keys(errors).length > 0) return { valid: false, errors };

  return {
    valid: true,
    input: {
      // The backend makes a lone issuer the account name.
      issuer: issuer === '' ? null : issuer,
      accountLabel,
      // As typed: the backend normalizes spaces, case and padding.
      secretBase32: form.secret,
      algorithm: form.algorithm,
      digits: Number(form.digits),
      ...scheme,
    },
  };
}

/**
 * Base32 as services show it: the letters A–Z (any case) and the digits 2–7,
 * with spaces anywhere and optional `=` padding at the end.
 */
function checkSecret(secret: string): string | null {
  const compact = compactSecret(secret);
  if (compact === '') return 'Enter the secret key.';
  if (!/^[a-z2-7]+$/i.test(compact)) {
    return 'A secret key has only the letters A–Z and the digits 2–7.';
  }
  return null;
}

/** The secret's base32 characters: without the spaces and padding services add. */
function compactSecret(secret: string): string {
  return secret.replace(/\s/g, '').replace(/=+$/, '');
}

/**
 * The shortest secret services are known to issue: 80 bits, 16 base32
 * characters (GitHub's, and the Key URI Format's own example). RFC 4226 §4
 * (R6) requires at least 128 bits and recommends 160, and RFC 6238 §5.1 the
 * HMAC's output length, but deployed 80-bit keys are common and the user
 * can't change what the service chose: a key shorter still is far more
 * likely cut short when copied than issued so.
 */
const SHORTEST_ISSUED_SECRET_BITS = 80;

/**
 * A warning for a valid secret shorter than services issue, most likely
 * copied only in part. It doesn't stop the entry being added.
 */
export function secretWarning(secret: string): string | null {
  if (checkSecret(secret) !== null) return null;
  const bits = compactSecret(secret).length * 5;
  if (bits >= SHORTEST_ISSUED_SECRET_BITS) return null;
  return `This key is shorter than services' keys (at least ${SHORTEST_ISSUED_SECRET_BITS / 5} characters). Check that you copied all of it.`;
}

type Scheme = Pick<TotpInput, 'type' | 'period'> | Pick<HotpInput, 'type' | 'counter'>;

interface SchemeError {
  field: 'period' | 'counter';
  error: string;
}

function readScheme(form: ManualEntryDraft): Scheme | SchemeError {
  if (form.type === 'totp') {
    const period = parseWholeNumber(form.period);
    if (period === null || period < PERIOD_SECONDS.min || period > PERIOD_SECONDS.max) {
      return {
        field: 'period',
        error: `Use a whole number of seconds from ${PERIOD_SECONDS.min} to ${PERIOD_SECONDS.max}.`,
      };
    }
    return { type: 'totp', period };
  }

  const counter = parseWholeNumber(form.counter);
  if (counter === null) return { field: 'counter', error: 'Use a whole number, 0 or more.' };
  return { type: 'hotp', counter };
}

const FIELD_FOR_KIND: Partial<Record<AppErrorKind, ManualEntryField>> = {
  MissingLabel: 'accountLabel',
  LabelContainsColon: 'accountLabel',
  InvalidSecret: 'secret',
  EmptySecret: 'secret',
  InvalidPeriod: 'period',
  CounterExhausted: 'counter',
};

/** The field a rejection of add_entry_manual is about, or null if it concerns the whole form. */
export function fieldForError(err: unknown): ManualEntryField | null {
  return isAppErrorPayload(err) ? (FIELD_FOR_KIND[err.kind] ?? null) : null;
}
