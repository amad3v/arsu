// The wire types of the IPC contract, mirrored from `crates/cmd/src/dto.rs`
// and `crates/cmd/src/error.rs`, which are the authority: a change there
// changes these in the same commit (src/api/contract.test.ts checks the
// error kinds).

export type OtpType = 'totp' | 'hotp';

export type Algorithm = 'sha1' | 'sha256' | 'sha512';

export type Theme = 'system' | 'light' | 'dark';

export type ImportFormat = 'aegis' | 'twofas';

/** The auto-lock delays the backend accepts, in minutes. */
export type AutoLockMinutes = 1 | 2 | 5 | 10 | 15 | 30;

export interface EntrySummary {
  /** UUID; pass back to getCurrentCode / deleteEntry / exportEntryQr. */
  id: string;
  issuer: string | null;
  /** Never empty. */
  accountLabel: string;
  otpType: OtpType;
  /** 6, 7 or 8. */
  digits: number;
  /** TOTP time step in seconds (1–300); null for HOTP. */
  period: number | null;
}

export interface CodeResponse {
  /** Digits as a string: leading zeros matter. */
  code: string;
  /** TOTP: whole seconds until `code` expires, 1..=period. Null for HOTP. */
  expiresInSeconds: number | null;
  /** TOTP: the code that replaces `code` when it expires. Null for HOTP. */
  nextCode: string | null;
}

interface ManualEntryBase {
  issuer: string | null;
  accountLabel: string;
  /** As typed: spaces, letter case and '=' padding don't matter. */
  secretBase32: string;
  algorithm: Algorithm;
  digits: number;
}

export interface TotpInput extends ManualEntryBase {
  type: 'totp';
  /** Seconds; 30 is the usual value. */
  period: number;
}

export interface HotpInput extends ManualEntryBase {
  type: 'hotp';
  /** The next counter to use, usually 0. */
  counter: number;
}

export type ManualEntryInput = TotpInput | HotpInput;

/** A file chosen in the native import dialog. The path stays in Rust. */
/** Whether the phone looks rooted, and whether the user accepted the risk. */
export interface DeviceCheck {
  rooted: boolean;
  /** Only ever true when `rooted` is. */
  accepted: boolean;
}

/** Biometric unlock: whether the platform has it, can use it now, and it is on. */
export interface BiometricStatus {
  /** This platform has biometric unlock (Android). */
  supported: boolean;
  /** A strong biometric (a fingerprint, or a secure face unlock) is set up and usable now. */
  available: boolean;
  /** Why not, when not. */
  reason: 'no-hardware' | 'not-enrolled' | 'unavailable' | 'update-required' | 'unsupported' | null;
  /** The user turned it on. */
  enabled: boolean;
}

/** What the About dialog shows. Paths are for display. */
export interface AboutInfo {
  name: string;
  version: string;
  tauriVersion: string;
  /** The engine the UI runs in: WebKitGTK on Linux, Android System WebView on Android. */
  webviewName: string;
  /** The WebView's version, or null if it can't be read. */
  webviewVersion: string | null;
  /** Where the vault is; null on Android, which shows no paths. */
  vaultPath: string | null;
  /** Where the settings are; null on Android. */
  settingsPath: string | null;
}

/** A project page the About dialog opens; the URLs are fixed in Rust. */
export type AppLink = 'website' | 'issues';

export interface PickedFile {
  /** Opaque; pass to importFile. */
  token: string;
  /** For display only. */
  fileName: string;
  /** The backup's format, told from its content; its name can be anything. */
  format: ImportFormat;
}

export interface SkippedEntry {
  /** How the source file names the entry, e.g. "GitHub (octocat)". */
  label: string;
  /** Why it was left out; written for the user, show as-is. */
  reason: string;
}

export interface ImportSummary {
  /** Ids of the new entries. */
  importedIds: string[];
  /** Entries that are not valid accounts. */
  skipped: SkippedEntry[];
  /** Valid accounts already in the vault, or repeated in the file. */
  duplicates: SkippedEntry[];
}

export interface Settings {
  theme: Theme;
  autoLockMinutes: AutoLockMinutes;
  /** 10..=60. */
  clipboardClearSeconds: number;
}

/** A change to the settings: each field present and not null replaces the current value. */
export interface SettingsUpdate {
  theme?: Theme | null;
  autoLockMinutes?: AutoLockMinutes | null;
  clipboardClearSeconds?: number | null;
}

/**
 * What every command rejects with. Branch on `kind`; show `message`, never
 * parse it (its wording is not part of the contract).
 */
export interface AppErrorPayload {
  kind: AppErrorKind;
  /** The error and its causes, for people. Never carries a secret. */
  message: string;
}

export type AppErrorKind =
  // The vault and its session
  | 'Locked' // the vault is locked: go to the unlock screen
  | 'VaultNotFound' // no vault exists yet: go to the create screen
  | 'VaultAlreadyExists' // a vault exists already: go to the unlock screen
  | 'VaultInUse' // another instance of the app has the vault open
  | 'WrongPassword' // the master password is wrong (on unlock also: the vault file is damaged)
  | 'WeakPassword' // a new password is shorter than 12 characters
  | 'NotAVault' // the vault file is not a vault
  | 'VaultTruncated' // the vault file is cut short
  | 'UnsupportedVaultVersion' // the vault file was written by a newer version of the app
  | 'UnsupportedKdfParams' // the vault file's key-derivation parameters are out of bounds
  | 'VaultEncode' // the vault could not be serialized for saving
  | 'VaultDecode' // the decrypted vault is malformed
  | 'KeyDerivation' // Argon2id failed
  | 'Encryption' // encryption failed (vault save, or an Aegis export)
  | 'Randomness' // the system random number generator failed
  | 'StorageIo' // reading or writing the vault or settings file failed
  | 'NoProjectDirs' // the data directory cannot be determined (startup only)
  // Settings
  | 'InvalidSettingsFile' // the settings file is not valid; the defaults are in effect
  | 'InvalidAutoLockMinutes' // not one of 1, 2, 5, 10, 15, 30
  | 'InvalidClipboardClearSeconds' // not within 10..=60
  // Entries
  | 'EntryNotFound' // no active entry with that id (deleted meanwhile): refresh the list
  | 'InvalidEntryId' // the id is not a UUID: a frontend bug
  // Accounts being added — from a URI, the manual form, or an import
  | 'NotOtpauth' // the text is not an otpauth:// URI
  | 'MissingLabel' // neither an issuer nor an account name
  | 'LabelContainsColon' // the issuer or account name contains ':'
  | 'InvalidLabelEncoding' // the URI's label is not valid UTF-8 once percent-decoded
  | 'MissingSecret' // the URI has no secret
  | 'InvalidSecret' // the secret is not base32
  | 'MissingCounter' // an HOTP URI without a counter
  | 'InvalidNumber' // a numeric URI parameter is not a number
  | 'UnknownOtpType' // neither TOTP nor HOTP
  | 'UnknownAlgorithm' // not SHA1, SHA256 or SHA512
  | 'InvalidDigits' // not 6, 7 or 8 digits
  | 'InvalidPeriod' // a TOTP period of 0 or more than 300 seconds
  | 'EmptySecret' // the secret decodes to nothing
  | 'CounterExhausted' // the HOTP counter is at its maximum and cannot advance
  | 'QrEncode' // the entry is too long to fit in a QR code
  // The clipboard
  | 'InvalidCode' // copyCode was given something other than 6–8 ASCII digits: a frontend bug
  | 'Clipboard' // the system clipboard could not be written
  // Import and export files
  | 'NoPendingImport' // the token is not the latest picked file (or was imported already)
  | 'NotAFile' // the picked path is not a regular file
  | 'FileRead' // the picked file could not be read
  | 'FileWrite' // the export could not be written where the user chose
  | 'InvalidFilePath' // the dialog returned a location that is not a local file
  | 'FileDialog' // the file dialog could not be shown
  | 'OpenLink' // no application opened the link
  | 'UnsupportedFileType' // the picked file's content is neither an Aegis nor a 2FAS backup
  | 'FileTooLarge' // over the 64 MiB import limit
  | 'UnrecognizedFormat' // not a file of the chosen format
  | 'UnsupportedFileVersion' // a version of the format this app doesn't read
  | 'PasswordRequired' // the file is encrypted: ask for its password and retry
  | 'WrongPasswordOrCorrupted' // the file's password is wrong, or the file is damaged
  | 'UnsupportedCredential' // an Aegis vault with no password slot
  | 'TooManySlots' // an Aegis vault with more key slots than any real one
  | 'UnsupportedFileKdfParams' // the file asks for key-derivation work beyond the app's bounds
  | 'MalformedFile' // the file's encryption header is malformed
  | 'ExportSerialize' // the export could not be serialized
  // Biometric unlock
  | 'BiometricCancelled' // the user dismissed the fingerprint/face prompt
  | 'BiometricLockout' // too many failed attempts; biometrics are locked for now
  | 'BiometricInvalidated' // it was turned off: the phone's biometrics or the vault changed
  | 'BiometricUnavailable' // no strong biometric is set up, or this platform has none
  | 'BiometricFailed' // the prompt or the keystore failed
  | 'BiometricNotEnabled' // biometric unlock is not turned on
  // The runtime
  | 'SystemClock' // the system clock is set before 1970
  | 'BackgroundTask'; // a backend task failed unexpectedly
