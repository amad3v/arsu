import { emit } from '@tauri-apps/api/event';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it, vi } from 'vitest';

import * as api from '@api';

import appCommandsRs from '../../src-tauri/src/app_commands.rs?raw';
import { rustBlock, rustListItems } from '../test/rust-source';

import type { InvokeArgs } from '@tauri-apps/api/core';

interface Call {
  cmd: string;
  args: InvokeArgs | undefined;
}

/** Records every invoke() and answers each with `reply`. */
function recordCalls(reply: unknown = null): Call[] {
  const calls: Call[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    return reply;
  });
  return calls;
}

const totp = {
  issuer: null,
  accountLabel: 'alice',
  secretBase32: 'JBSWY3DPEHPK3PXP',
  algorithm: 'sha1',
  digits: 6,
  type: 'totp',
  period: 30,
} as const;

// Command names and argument names (camelCase) are the wire contract with
// crates/cmd/src/commands.rs; each case is one wrapper.
const wrappers: [string, () => Promise<unknown>, string, InvokeArgs | undefined][] = [
  ['vaultExists', () => api.vaultExists(), 'vault_exists', undefined],
  ['createVault', () => api.createVault('pw'), 'create_vault', { masterPassword: 'pw' }],
  ['unlockVault', () => api.unlockVault('pw'), 'unlock_vault', { masterPassword: 'pw' }],
  ['lockVault', () => api.lockVault(), 'lock_vault', undefined],
  ['isUnlocked', () => api.isUnlocked(), 'is_unlocked', undefined],
  ['recordActivity', () => api.recordActivity(), 'record_activity', undefined],
  ['deviceCheck', () => api.deviceCheck(), 'device_check', undefined],
  ['answerRootWarning', () => api.answerRootWarning(true), 'answer_root_warning', { accept: true }],
  ['biometricStatus', () => api.biometricStatus(), 'biometric_status', undefined],
  [
    'enableBiometricUnlock',
    () => api.enableBiometricUnlock('pw'),
    'enable_biometric_unlock',
    { masterPassword: 'pw' },
  ],
  [
    'disableBiometricUnlock',
    () => api.disableBiometricUnlock(),
    'disable_biometric_unlock',
    undefined,
  ],
  ['unlockWithBiometric', () => api.unlockWithBiometric(), 'unlock_with_biometric', undefined],
  ['listEntries', () => api.listEntries(), 'list_entries', undefined],
  ['getCurrentCode', () => api.getCurrentCode('id-1'), 'get_current_code', { entryId: 'id-1' }],
  ['copyCode', () => api.copyCode('123456'), 'copy_code', { code: '123456' }],
  [
    'addEntryFromUri',
    () => api.addEntryFromUri('otpauth://x'),
    'add_entry_from_uri',
    { uri: 'otpauth://x' },
  ],
  ['addEntryManual', () => api.addEntryManual(totp), 'add_entry_manual', { input: totp }],
  ['deleteEntry', () => api.deleteEntry('id-1'), 'delete_entry', { entryId: 'id-1' }],
  [
    'exportEntryQr',
    () => api.exportEntryQr('id-1', 'pw'),
    'export_entry_qr',
    { entryId: 'id-1', masterPassword: 'pw' },
  ],
  [
    'exportEntryQrWithBiometric',
    () => api.exportEntryQrWithBiometric('id-1'),
    'export_entry_qr_with_biometric',
    { entryId: 'id-1' },
  ],
  ['pickImportFile', () => api.pickImportFile(), 'pick_import_file', undefined],
  [
    'importFile',
    () => api.importFile('tok', null),
    'import_file',
    { token: 'tok', password: null },
  ],
  [
    'exportToAegisFile',
    () => api.exportToAegisFile('pw'),
    'export_to_aegis_file',
    { password: 'pw' },
  ],
  ['pickQrImage', () => api.pickQrImage(), 'pick_qr_image', undefined],
  ['getSettings', () => api.getSettings(), 'get_settings', undefined],
  ['getAboutInfo', () => api.getAboutInfo(), 'get_about_info', undefined],
  ['copyAboutDetails', () => api.copyAboutDetails(), 'copy_about_details', undefined],
  ['openLink', () => api.openLink('issues'), 'open_link', { link: 'issues' }],
  ['showWindow', () => api.showWindow(), 'show_window', undefined],
  [
    'updateSettings',
    () => api.updateSettings({ autoLockMinutes: 10 }),
    'update_settings',
    { update: { autoLockMinutes: 10 } },
  ],
];

describe('command wrappers', () => {
  it.each(wrappers)(
    '%s invokes %s with the documented arguments',
    async (_name, call, cmd, args) => {
      const calls = recordCalls();

      await call();

      expect(calls).toEqual([{ cmd, args: args ?? {} }]);
    },
  );

  it('cover exactly the commands the backend registers', () => {
    // src-tauri/src/app_commands.rs is the backend's one list of commands.
    const registered = rustListItems(rustBlock(appCommandsRs, '$callback! {'));

    expect(wrappers.map(([, , cmd]) => cmd).sort()).toEqual(registered.sort());
  });

  it('resolve to what the backend returns', async () => {
    const code = { code: '012345', expiresInSeconds: 12, nextCode: '678901' };
    recordCalls(code);

    await expect(api.getCurrentCode('id-1')).resolves.toEqual(code);
  });

  it('decode the picked QR code image from base64', async () => {
    recordCalls(btoa('\x89PNG'));
    const image = await api.pickQrImage();
    expect(new Uint8Array(await image!.arrayBuffer())).toEqual(
      Uint8Array.of(0x89, 0x50, 0x4e, 0x47),
    );

    recordCalls(null);
    await expect(api.pickQrImage()).resolves.toBeNull();
  });
});

describe('onVaultLocked', () => {
  it('calls the handler for each vault-locked event until unsubscribed', async () => {
    mockIPC(() => null, { shouldMockEvents: true });
    const handler = vi.fn();

    const unlisten = await api.onVaultLocked(handler);
    await emit(api.VAULT_LOCKED_EVENT, null);
    expect(handler).toHaveBeenCalledTimes(1);

    unlisten();
    await emit(api.VAULT_LOCKED_EVENT, null);
    expect(handler).toHaveBeenCalledTimes(1);
  });

  it('ignores other events', async () => {
    mockIPC(() => null, { shouldMockEvents: true });
    const handler = vi.fn();

    await api.onVaultLocked(handler);
    await emit('something-else', null);

    expect(handler).not.toHaveBeenCalled();
  });
});
