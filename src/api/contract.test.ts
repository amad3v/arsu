// Ties the TypeScript mirror of the IPC contract to the Rust source it
// mirrors, so a change on one side fails here instead of at runtime.

import { describe, expect, it } from 'vitest';

import { VAULT_LOCKED_EVENT } from '@api';

import autoLockRs from '../../crates/cmd/src/auto_lock.rs?raw';
import errorRs from '../../crates/cmd/src/error.rs?raw';
import passwordRs from '../../crates/cmd/src/password.rs?raw';
import settingsRs from '../../crates/storage/src/settings.rs?raw';
import { rustBlock, rustCapture, rustListItems } from '../test/rust-source';

import { MIN_PASSWORD_CHARS } from './password-policy';
import { AUTO_LOCK_MINUTES, CLIPBOARD_CLEAR_SECONDS, DEFAULT_SETTINGS } from './settings';

import type { AppErrorKind } from '@app-types/api';

// Typed as a Record so that tsc fails when this list and the AppErrorKind
// union differ: a missing kind is a missing key, an unknown one an excess key.
const everyKind: Record<AppErrorKind, true> = {
  Locked: true,
  VaultNotFound: true,
  VaultAlreadyExists: true,
  VaultInUse: true,
  WrongPassword: true,
  WeakPassword: true,
  NotAVault: true,
  VaultTruncated: true,
  UnsupportedVaultVersion: true,
  UnsupportedKdfParams: true,
  VaultEncode: true,
  VaultDecode: true,
  KeyDerivation: true,
  Encryption: true,
  Randomness: true,
  StorageIo: true,
  NoProjectDirs: true,
  InvalidSettingsFile: true,
  InvalidAutoLockMinutes: true,
  InvalidClipboardClearSeconds: true,
  EntryNotFound: true,
  InvalidEntryId: true,
  NotOtpauth: true,
  MissingLabel: true,
  LabelContainsColon: true,
  InvalidLabelEncoding: true,
  MissingSecret: true,
  InvalidSecret: true,
  MissingCounter: true,
  InvalidNumber: true,
  UnknownOtpType: true,
  UnknownAlgorithm: true,
  InvalidDigits: true,
  InvalidPeriod: true,
  EmptySecret: true,
  CounterExhausted: true,
  QrEncode: true,
  InvalidCode: true,
  Clipboard: true,
  NoPendingImport: true,
  NotAFile: true,
  FileRead: true,
  FileWrite: true,
  InvalidFilePath: true,
  FileDialog: true,
  OpenLink: true,
  UnsupportedFileType: true,
  FileTooLarge: true,
  UnrecognizedFormat: true,
  UnsupportedFileVersion: true,
  PasswordRequired: true,
  WrongPasswordOrCorrupted: true,
  UnsupportedCredential: true,
  TooManySlots: true,
  UnsupportedFileKdfParams: true,
  MalformedFile: true,
  ExportSerialize: true,
  BiometricCancelled: true,
  BiometricLockout: true,
  BiometricInvalidated: true,
  BiometricUnavailable: true,
  BiometricFailed: true,
  BiometricNotEnabled: true,
  SystemClock: true,
  BackgroundTask: true,
};

describe('the TypeScript contract matches the Rust source', () => {
  it('AppErrorKind lists exactly the variants of cmd::error::AppErrorKind', () => {
    const variants = rustListItems(rustBlock(errorRs, 'pub enum AppErrorKind'));

    expect(Object.keys(everyKind).sort()).toEqual(variants.sort());
  });

  it('the auto-lock event is cmd::auto_lock::VAULT_LOCKED_EVENT', () => {
    const rust = rustCapture(autoLockRs, /pub const VAULT_LOCKED_EVENT: &str = "([^"]+)";/);

    expect(VAULT_LOCKED_EVENT).toBe(rust);
  });

  it('the password minimum is cmd::password::MIN_PASSWORD_CHARS', () => {
    const rust = rustCapture(passwordRs, /pub const MIN_PASSWORD_CHARS: usize = (\d+);/);

    expect(MIN_PASSWORD_CHARS).toBe(Number(rust));
  });

  describe('settings (storage::settings)', () => {
    it('offers exactly the auto-lock delays the backend accepts', () => {
      const allowed = rustCapture(settingsRs, /pub const ALLOWED: \[u32; \d+\] = \[([^\]]*)\];/)
        .split(',')
        .map((n) => Number(n.trim()));

      expect(AUTO_LOCK_MINUTES).toEqual(allowed);
    });

    it('bounds the clipboard delay as the backend does', () => {
      const [min, max] = rustCapture(
        settingsRs,
        /pub const RANGE: RangeInclusive<u32> = (\d+\.\.=\d+);/,
      )
        .split('..=')
        .map(Number);

      expect(CLIPBOARD_CLEAR_SECONDS).toEqual({ min, max });
    });

    it('uses the backend defaults', () => {
      // `[^}]*` keeps each match inside the first lines of its own impl block.
      const autoLock = rustCapture(
        settingsRs,
        /impl AutoLockMinutes \{[^}]*pub const DEFAULT: Self = Self\((\d+)\);/,
      );
      const clipboard = rustCapture(
        settingsRs,
        /impl ClipboardClearSeconds \{[^}]*pub const DEFAULT: Self = Self\((\d+)\);/,
      );

      expect(DEFAULT_SETTINGS).toEqual({
        theme: 'system',
        autoLockMinutes: Number(autoLock),
        clipboardClearSeconds: Number(clipboard),
      });
    });
  });
});
