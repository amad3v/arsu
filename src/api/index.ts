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
  BiometricStatus,
  CodeResponse,
  DeviceCheck,
  DiagnosticReportPreview,
  EntrySummary,
  ImportSummary,
  ManualEntryInput,
  NearbyDevice,
  PickedFile,
  Settings,
  SettingsUpdate,
  SyncInvitation,
  SyncOverview,
  SyncSessionReport,
  SyncSummary,
  UiError,
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

// ─── The device ──────────────────────────────────────────────────────────────

/** Whether the phone looks rooted (never on the desktop), and whether the user accepted the risk. */
export function deviceCheck(): Promise<DeviceCheck> {
  return invoke<DeviceCheck>('device_check');
}

/** Records the user's consent to a rooted phone's risk, or, if they refuse, closes the app. */
export function answerRootWarning(accept: boolean): Promise<void> {
  return invoke('answer_root_warning', { accept });
}

// ─── Biometric unlock ────────────────────────────────────────────────────────

/** Whether biometric unlock is supported here, usable now, and turned on. */
export function biometricStatus(): Promise<BiometricStatus> {
  return invoke<BiometricStatus>('biometric_status');
}

/**
 * Turns biometric unlock on: re-checks the master password (Argon2id), then
 * the system asks for a fingerprint or face. The vault must be unlocked.
 */
export function enableBiometricUnlock(masterPassword: string): Promise<void> {
  return invoke('enable_biometric_unlock', { masterPassword });
}

export function disableBiometricUnlock(): Promise<void> {
  return invoke('disable_biometric_unlock');
}

/** The system asks for a fingerprint or face, then the vault unlocks. */
export function unlockWithBiometric(): Promise<void> {
  return invoke('unlock_with_biometric');
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

/**
 * As exportEntryQr, with the fingerprint or face in place of the master
 * password: the system asks for it first. Needs biometric unlock turned on.
 */
export function exportEntryQrWithBiometric(entryId: string): Promise<string> {
  return invoke<string>('export_entry_qr_with_biometric', { entryId });
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

// ─── Sync ────────────────────────────────────────────────────────────────────

/** This device and the other devices of the vault; sets sync up on first use. */
export function getSyncOverview(): Promise<SyncOverview> {
  return invoke<SyncOverview>('get_sync_overview');
}

/** Removes a device from the vault, for good. It keeps what it already has. */
export function removeSyncDevice(deviceId: string): Promise<void> {
  return invoke('remove_sync_device', { deviceId });
}

/**
 * Asks where to save, then writes a sync file for a paired device. Resolves
 * to false if the user cancels the save dialog.
 */
export function exportSyncFile(deviceId: string): Promise<boolean> {
  return invoke<boolean>('export_sync_file', { deviceId });
}

/**
 * Asks where to save, then writes a join file for the device whose
 * invitation QR code reads `invitation`. Resolves to false if the user cancels.
 */
export function exportJoinFile(invitation: string): Promise<boolean> {
  return invoke<boolean>('export_join_file', { invitation });
}

/**
 * Opens the native file dialog for a sync or join file, and merges it.
 * Resolves to null if the user cancels.
 */
export function importSyncFile(): Promise<SyncSummary | null> {
  return invoke<SyncSummary | null>('import_sync_file');
}

// ─── Live sync over Bluetooth ────────────────────────────────────────────────

/**
 * Starts serving paired devices over Bluetooth until stopBluetooth; each
 * sync is reported by the sync-session event (see onSyncSession).
 */
export function startBluetoothSync(): Promise<void> {
  return invoke('start_bluetooth_sync');
}

/**
 * Opens an invitation and waits over Bluetooth for the device that reads it;
 * the pairing is reported by the sync-session event. Resolves to the invitation to show.
 */
export function startBluetoothPairing(): Promise<SyncInvitation> {
  return invoke<SyncInvitation>('start_bluetooth_pairing');
}

/** Stops Bluetooth sync or pairing; a session in progress merges nothing. */
export function stopBluetooth(): Promise<void> {
  return invoke('stop_bluetooth');
}

/** The Arsu devices nearby, after a short Bluetooth scan. */
export function findBluetoothDevices(): Promise<NearbyDevice[]> {
  return invoke<NearbyDevice[]>('find_bluetooth_devices');
}

/** Syncs with a paired device nearby, by the address findBluetoothDevices gave. */
export function syncOverBluetooth(address: string): Promise<SyncSummary> {
  return invoke<SyncSummary>('sync_over_bluetooth', { address });
}

/** Pairs with the nearby device showing `invitation`: its QR code's text, or the typed code. */
export function joinOverBluetooth(invitation: string): Promise<SyncSummary> {
  return invoke<SyncSummary>('join_over_bluetooth', { invitation });
}

/** The event live sessions report their outcome with: cmd::bluetooth::SYNC_SESSION_EVENT. */
export const SYNC_SESSION_EVENT = 'sync-session';

/** Calls `handler` with each live session's outcome. Resolves to the function that stops listening. */
export function onSyncSession(handler: (report: SyncSessionReport) => void): Promise<UnlistenFn> {
  return listen<SyncSessionReport>(SYNC_SESSION_EVENT, (event) => {
    handler(event.payload);
  });
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

/**
 * Prepares a diagnostic report of the log as it is now (what went wrong,
 * never a secret), and returns what it holds, to show before it is saved.
 */
export function prepareDiagnosticReport(): Promise<DiagnosticReportPreview> {
  return invoke<DiagnosticReportPreview>('prepare_diagnostic_report');
}

/**
 * Saves the report prepareDiagnosticReport last prepared, as shown, to a
 * location the user chooses, for a bug report. False if they cancel.
 */
export function saveDiagnosticReport(): Promise<boolean> {
  return invoke<boolean>('save_diagnostic_report');
}

/** Writes an error the interface ran into to the diagnostic log. */
export function reportUiError(error: UiError): Promise<void> {
  return invoke('report_ui_error', { error });
}

/** Opens one of the project's pages in the user's browser. */
export function openLink(link: AppLink): Promise<void> {
  return invoke('open_link', { link });
}

// ─── Window ──────────────────────────────────────────────────────────────────

/**
 * Shows the window, which starts hidden so that the WebView's blank white page
 * never flashes on screen: call it once the first render (in the right theme) is
 * in the DOM.
 */
export function showWindow(): Promise<void> {
  return invoke('show_window');
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
