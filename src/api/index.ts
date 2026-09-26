// The only module that talks to Rust: one typed wrapper per command in
// crates/cmd/src/commands.rs (whose docs list each one's errors), plus the
// one event the backend emits. Every promise rejects with an AppErrorPayload
// (see ./lib.ts for the helpers).

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import { base64ToBytes } from './lib';

import type {
  AboutInfo,
  AppLink,
  CodeResponse,
  EntrySummary,
  ImportSummary,
  ManualEntryInput,
  PickedFile,
  Settings,
  SettingsUpdate,
} from '@app-types/api';
import type { UnlistenFn } from '@tauri-apps/api/event';

// ─── Vault lifecycle ─────────────────────────────────────────────────────────

export function vaultExists(): Promise<boolean> {
  return invoke<boolean>('vault_exists');
}

/** Creates the vault and leaves it unlocked. Slow by design (Argon2id). */
export function createVault(masterPassword: string): Promise<void> {
  return invoke('create_vault', { masterPassword });
}

/** Slow by design (Argon2id). */
export function unlockVault(masterPassword: string): Promise<void> {
  return invoke('unlock_vault', { masterPassword });
}

/** The explicit "lock" action. No `vault-locked` event follows it. */
export function lockVault(): Promise<void> {
  return invoke('lock_vault');
}

export function isUnlocked(): Promise<boolean> {
  return invoke<boolean>('is_unlocked');
}

/** Postpones the backend's auto-lock. Call it throttled, on user input. */
export function recordActivity(): Promise<void> {
  return invoke('record_activity');
}

// ─── Entries and codes ───────────────────────────────────────────────────────

/** Active entries, in the order they were added: the caller sorts. */
export function listEntries(): Promise<EntrySummary[]> {
  return invoke<EntrySummary[]>('list_entries');
}

/**
 * The entry's current code. For HOTP this uses a code up (the counter
 * advances and is saved), so call it only when the user asks.
 */
export function getCurrentCode(entryId: string): Promise<CodeResponse> {
  return invoke<CodeResponse>('get_current_code', { entryId });
}

/**
 * Rust writes `code` to the system clipboard and clears it after the
 * `clipboardClearSeconds` setting, if the clipboard still holds it.
 */
export function copyCode(code: string): Promise<void> {
  return invoke('copy_code', { code });
}

/** Resolves to the new entry's id. */
export function addEntryFromUri(uri: string): Promise<string> {
  return invoke<string>('add_entry_from_uri', { uri });
}

/** Resolves to the new entry's id. */
export function addEntryManual(input: ManualEntryInput): Promise<string> {
  return invoke<string>('add_entry_manual', { input });
}

export function deleteEntry(entryId: string): Promise<void> {
  return invoke('delete_entry', { entryId });
}

/**
 * An SVG document of a QR code that encodes the entry's otpauth:// URI,
 * secret included. The backend re-verifies the master password first.
 * Render it only through `<img>` (see svgToDataUri) and drop it when the
 * dialog closes.
 */
export function exportEntryQr(entryId: string, masterPassword: string): Promise<string> {
  return invoke<string>('export_entry_qr', { entryId, masterPassword });
}

// ─── Import and export ───────────────────────────────────────────────────────

/** Opens the native file dialog for an Aegis or 2FAS backup. Resolves to null if the user cancels. */
export function pickImportFile(): Promise<PickedFile | null> {
  return invoke<PickedFile | null>('pick_import_file');
}

/**
 * Imports the picked file. Pass `null` first, and the file's password on a
 * retry after `PasswordRequired`; the token stays valid until an import succeeds.
 */
export function importFile(token: string, password: string | null): Promise<ImportSummary> {
  return invoke<ImportSummary>('import_file', { token, password });
}

/**
 * Asks where to save, then writes an encrypted Aegis backup. Resolves to
 * false if the user cancels the save dialog.
 */
export function exportToAegisFile(password: string): Promise<boolean> {
  return invoke<boolean>('export_to_aegis_file', { password });
}

/**
 * Opens the native file dialog for a QR code image. Resolves to the image,
 * or to null if the user cancels.
 */
export async function pickQrImage(): Promise<Blob | null> {
  const base64 = await invoke<string | null>('pick_qr_image');
  return base64 === null ? null : new Blob([base64ToBytes(base64)]);
}

// ─── Settings ────────────────────────────────────────────────────────────────

export function getSettings(): Promise<Settings> {
  return invoke<Settings>('get_settings');
}

/** Resolves to every setting after the change; nothing changes unless it is saved. */
export function updateSettings(update: SettingsUpdate): Promise<Settings> {
  return invoke<Settings>('update_settings', { update });
}

// ─── About ───────────────────────────────────────────────────────────────────

export function getAboutInfo(): Promise<AboutInfo> {
  return invoke<AboutInfo>('get_about_info');
}

/** Copies the About dialog's details (versions and paths) to the clipboard, for a bug report. */
export function copyAboutDetails(): Promise<void> {
  return invoke('copy_about_details');
}

/** Opens one of the project's pages in the user's browser. */
export function openLink(link: AppLink): Promise<void> {
  return invoke('open_link', { link });
}

// ─── Events ──────────────────────────────────────────────────────────────────

/** Emitted when the backend auto-locks the vault after the user was idle. */
export const VAULT_LOCKED_EVENT = 'vault-locked';

/**
 * Calls `handler` each time the backend auto-locks the vault (never for an
 * explicit lockVault). Resolves to the function that stops listening.
 */
export function onVaultLocked(handler: () => void): Promise<UnlistenFn> {
  return listen<null>(VAULT_LOCKED_EVENT, () => {
    handler();
  });
}
