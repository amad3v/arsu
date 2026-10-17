import { fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library';
import { emit } from '@tauri-apps/api/event';
import { createSignal } from 'solid-js';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { scanQrInWorker } from '@cpt/add-entry-dialog/scan-qr';
import { dialogSettled } from '@cpt/testing/dialog';
import { appError, mockCommands } from '@cpt/testing/ipc';

import { SyncDialog } from '.';

import type { CommandHandler } from '@cpt/testing/ipc';

vi.mock('@cpt/add-entry-dialog/scan-qr', () => ({ scanQrInWorker: vi.fn() }));

const thisDevice = {
  id: 'aa'.repeat(32),
  name: 'Computer',
  fingerprint: 'AAAA AAAA AAAA AAAA',
  addedAt: 1,
};
const phone = {
  id: 'bb'.repeat(32),
  name: 'Pixel',
  fingerprint: 'BBBB BBBB BBBB BBBB',
  addedAt: 2,
};
const overview = { thisDevice, devices: [phone] };
const synced = { peerName: 'Pixel', added: 2, updated: 0, deleted: 0, paired: false };
const invitation = { qrSvg: '<svg/>', code: 'ABCDE-FGHJK', expiresAt: 1_760_000_900 };
const QR_TEXT = 'ARSU-INVITE:0123456789';
/** A whole typed code: 55 characters in groups of five. */
const CODE = 'ABCDE-FGHJK-MNPQR-STVWX-YZ012-34567-89ABC-DEFGH-JKMNP-QRSTV-WXYZ0';

function renderDialog(handlers: Partial<Record<string, CommandHandler>> = {}) {
  const calls = mockCommands(
    { get_sync_overview: () => overview, stop_bluetooth: () => null, ...handlers },
    { events: true },
  );
  const onSynced = vi.fn();
  const [open, setOpen] = createSignal(true);
  render(() => <SyncDialog open={open()} onClose={() => setOpen(false)} onSynced={onSynced} />);
  const called = (cmd: string) => calls.filter((call) => call.cmd === cmd);
  return { calls, called, onSynced, close: () => setOpen(false) };
}

beforeEach(() => {
  vi.mocked(scanQrInWorker).mockReset();
});

/** A button once it can be pressed: the sync actions wait for the device list. */
async function ready(name: string): Promise<HTMLButtonElement> {
  const button = await screen.findByRole<HTMLButtonElement>('button', { name });
  await waitFor(() => {
    expect(button.disabled).toBe(false);
  });
  return button;
}

describe('the device list', () => {
  it('shows this device and the paired ones, with their fingerprints', async () => {
    renderDialog();

    expect(await screen.findByText('Computer')).toBeTruthy();
    expect(screen.getByText('AAAA AAAA AAAA AAAA')).toBeTruthy();
    expect(screen.getByText('Pixel')).toBeTruthy();
    expect(screen.getByText('BBBB BBBB BBBB BBBB')).toBeTruthy();
  });

  it('says when no other device is paired yet', async () => {
    renderDialog({ get_sync_overview: () => ({ thisDevice, devices: [] }) });

    expect(await screen.findByText(/No other device yet/)).toBeTruthy();
  });

  it('offers syncing only once a device is paired', async () => {
    renderDialog({ get_sync_overview: () => ({ thisDevice, devices: [] }) });

    const nearby = await screen.findByRole<HTMLButtonElement>('button', {
      name: 'Sync with a device nearby',
    });
    expect(nearby.disabled).toBe(true);
    expect(
      screen.getByRole<HTMLButtonElement>('button', { name: 'Wait for my devices' }).disabled,
    ).toBe(true);
    // Pairing and files stay open.
    expect(
      screen.getByRole<HTMLButtonElement>('button', { name: 'Pair a new device' }).disabled,
    ).toBe(false);
  });

  it('says why syncing is unavailable before any device is paired', async () => {
    renderDialog({ get_sync_overview: () => ({ thisDevice, devices: [] }) });

    const reason = /Pair a device first/;
    expect(
      await screen.findByRole('button', { name: 'Sync with a device nearby', description: reason }),
    ).toBeTruthy();
    expect(
      screen.getByRole('button', { name: 'Wait for my devices', description: reason }),
    ).toBeTruthy();
    expect(screen.getByText(reason)).toBeTruthy();
  });

  it('drops the reason once a device is paired', async () => {
    renderDialog();

    expect(await screen.findByText('Pixel')).toBeTruthy();
    expect(screen.queryByText(/Pair a device first/)).toBeNull();
  });

  it('says where sync files come from before any device is paired', async () => {
    renderDialog({ get_sync_overview: () => ({ thisDevice, devices: [] }) });

    expect(await screen.findByText(/Sync file button/)).toBeTruthy();
  });

  it('points out two devices with one name, as a reinstall leaves', async () => {
    const reinstalled = { ...phone, id: 'cc'.repeat(32), fingerprint: 'CCCC CCCC CCCC CCCC' };
    renderDialog({ get_sync_overview: () => ({ thisDevice, devices: [phone, reinstalled] }) });

    expect(await screen.findAllByText(/Same name as another device/)).toHaveLength(2);
    // Each can be removed from there, the old one most likely.
    expect(screen.getAllByRole('button', { name: 'Remove Pixel' })).toHaveLength(2);
  });

  it('points out a device with this device’s name', async () => {
    const earlier = { ...phone, name: 'Computer' };
    renderDialog({ get_sync_overview: () => ({ thisDevice, devices: [earlier] }) });

    expect(await screen.findByText(/Same name as this device/)).toBeTruthy();
  });

  it('says nothing of devices with names of their own', async () => {
    renderDialog();

    expect(await screen.findByText('Pixel')).toBeTruthy();
    expect(screen.queryByText(/Same name as/)).toBeNull();
  });

  it('marks a device still waiting for its reply', async () => {
    renderDialog({
      get_sync_overview: () => ({
        thisDevice,
        devices: [{ ...phone, addedAt: JSON.parse('18446744073709551615') as number }],
      }),
    });

    expect(await screen.findByText('Waiting for its reply')).toBeTruthy();
  });

  it('removes a device once the removal is confirmed', async () => {
    const { called } = renderDialog({ remove_sync_device: () => null });

    fireEvent.click(await screen.findByRole('button', { name: 'Remove Pixel' }));
    expect(called('remove_sync_device')).toHaveLength(0);
    fireEvent.click(screen.getByRole('button', { name: 'Remove Pixel for good' }));

    await waitFor(() => expect(called('get_sync_overview')).toHaveLength(2));
    expect(called('remove_sync_device')[0].args).toEqual({ deviceId: phone.id });
  });

  it('saves a sync file for a device', async () => {
    const { called } = renderDialog({ export_sync_file: () => true });

    fireEvent.click(await screen.findByRole('button', { name: 'Save a sync file for Pixel' }));

    expect(await screen.findByText(/Sync file saved/)).toBeTruthy();
    expect(called('export_sync_file')[0].args).toEqual({ deviceId: phone.id });
  });
});

describe('syncing', () => {
  it('opens a sync file and says what changed', async () => {
    const { onSynced } = renderDialog({ import_sync_file: () => synced });

    fireEvent.click(await screen.findByRole('button', { name: 'Open a sync file' }));

    expect(await screen.findByText('Synced with Pixel: 2 accounts added.')).toBeTruthy();
    expect(onSynced).toHaveBeenCalledOnce();
  });

  it('changes nothing when the file dialog is cancelled', async () => {
    const { onSynced } = renderDialog({ import_sync_file: () => null });

    fireEvent.click(await screen.findByRole('button', { name: 'Open a sync file' }));

    await waitFor(() => expect(onSynced).not.toHaveBeenCalled());
    expect(screen.queryByRole('status')).toBeNull();
  });

  it('syncs at once with the one device waiting nearby', async () => {
    const { called, onSynced } = renderDialog({
      find_bluetooth_devices: () => [{ address: 'AA:BB:CC:DD:EE:FF', pairing: false }],
      sync_over_bluetooth: () => synced,
    });

    fireEvent.click(await ready('Sync with a device nearby'));

    expect(await screen.findByText('Synced with Pixel: 2 accounts added.')).toBeTruthy();
    expect(called('sync_over_bluetooth')[0].args).toEqual({ address: 'AA:BB:CC:DD:EE:FF' });
    expect(onSynced).toHaveBeenCalledOnce();
  });

  it('lets the user choose between several devices waiting', async () => {
    const { called } = renderDialog({
      find_bluetooth_devices: () => [
        { address: 'AA:BB:CC:DD:EE:FF', pairing: false },
        { address: '11:22:33:44:55:66', pairing: false },
      ],
      sync_over_bluetooth: () => synced,
    });

    fireEvent.click(await ready('Sync with a device nearby'));
    fireEvent.click(await screen.findByRole('button', { name: 'Sync with 11:22:33:44:55:66' }));

    expect(await screen.findByText('Synced with Pixel: 2 accounts added.')).toBeTruthy();
    expect(called('sync_over_bluetooth').map((call) => call.args)).toEqual([
      { address: '11:22:33:44:55:66' },
    ]);
    expect(screen.getByText(/Only devices paired with your vault/)).toBeTruthy();
  });

  it('does not take a device waiting to pair for one waiting to sync', async () => {
    const { called } = renderDialog({
      find_bluetooth_devices: () => [{ address: 'AA:BB:CC:DD:EE:FF', pairing: true }],
    });

    fireEvent.click(await ready('Sync with a device nearby'));

    expect(await screen.findByText(/No device found nearby/)).toBeTruthy();
    expect(called('sync_over_bluetooth')).toHaveLength(0);
  });

  it('says how to find a device when none is nearby', async () => {
    renderDialog({ find_bluetooth_devices: () => [] });

    fireEvent.click(await ready('Sync with a device nearby'));

    expect(await screen.findByText(/Wait for my devices/)).toBeTruthy();
    expect(screen.queryByText(/Android 11 and earlier/)).toBeNull();
  });

  it('waits for paired devices and reports each sync', async () => {
    const { called, onSynced } = renderDialog({ start_bluetooth_sync: () => null });

    fireEvent.click(await ready('Wait for my devices'));
    await waitFor(() => expect(called('start_bluetooth_sync')).toHaveLength(1));
    await emit('sync-session', { kind: 'finished', summary: synced });

    expect(await screen.findByText('Synced with Pixel: 2 accounts added.')).toBeTruthy();
    expect(onSynced).toHaveBeenCalledOnce();
  });
});

describe('pairing', () => {
  it('shows an invitation, serves it over Bluetooth and reports the pairing', async () => {
    const { called, onSynced } = renderDialog({ start_bluetooth_pairing: () => invitation });

    fireEvent.click(await screen.findByRole('button', { name: 'Pair a new device' }));

    const qr = await screen.findByRole<HTMLImageElement>('img', { name: /invitation/i });
    expect(qr.src).toMatch(/^data:image\/svg\+xml/);
    expect(screen.getByText('ABCDE-FGHJK')).toBeTruthy();
    expect(called('start_bluetooth_pairing')).toHaveLength(1);

    await emit('sync-session', { kind: 'finished', summary: { ...synced, paired: true } });

    expect(await screen.findByText('Paired with Pixel: 2 accounts added.')).toBeTruthy();
    expect(onSynced).toHaveBeenCalledOnce();
  });

  it('stops Bluetooth when the dialog closes', async () => {
    const { called, close } = renderDialog({ start_bluetooth_pairing: () => invitation });
    fireEvent.click(await screen.findByRole('button', { name: 'Pair a new device' }));
    await screen.findByText('ABCDE-FGHJK');

    close();

    await waitFor(() => expect(called('stop_bluetooth')).toHaveLength(1));
  });

  it('joins over Bluetooth with a typed code, once confirmed', async () => {
    const { called, onSynced } = renderDialog({
      join_over_bluetooth: () => ({ ...synced, paired: true }),
    });
    fireEvent.click(await screen.findByRole('button', { name: 'Join with an invitation' }));

    // The whole code asks at once, without another tap.
    fireEvent.input(screen.getByLabelText('Or type its code'), { target: { value: CODE } });

    const confirmation = await screen.findByRole('dialog', { name: 'Pair with this device?' });
    // A typed code has no key to write a join file to.
    expect(
      within(confirmation).queryByRole('button', { name: 'Save a join file instead' }),
    ).toBeNull();
    expect(within(confirmation).getByText(/needs its QR code/)).toBeTruthy();
    fireEvent.click(within(confirmation).getByRole('button', { name: 'Pair over Bluetooth' }));

    expect(await screen.findByText('Paired with Pixel: 2 accounts added.')).toBeTruthy();
    expect(called('join_over_bluetooth')[0].args).toEqual({ invitation: CODE });
    expect(onSynced).toHaveBeenCalledOnce();
  });

  it('waits for the whole code before asking', async () => {
    renderDialog();
    fireEvent.click(await screen.findByRole('button', { name: 'Join with an invitation' }));

    const field = screen.getByLabelText('Or type its code');
    fireEvent.input(field, { target: { value: CODE.slice(0, -1) } });
    fireEvent.keyDown(field, { key: 'Enter' });

    expect(screen.queryByRole('dialog', { name: 'Pair with this device?' })).toBeNull();
  });

  it('asks again for a typed code on Enter, once cancelled', async () => {
    renderDialog();
    const join = await screen.findByRole('button', { name: 'Join with an invitation' });
    await dialogSettled();
    fireEvent.click(join);
    const field = screen.getByLabelText('Or type its code');
    fireEvent.input(field, { target: { value: CODE } });
    fireEvent.click(await screen.findByRole('button', { name: 'Cancel' }));
    await waitFor(() =>
      expect(screen.queryByRole('dialog', { name: 'Pair with this device?' })).toBeNull(),
    );

    fireEvent.keyDown(field, { key: 'Enter' });

    expect(await screen.findByRole('dialog', { name: 'Pair with this device?' })).toBeTruthy();
  });

  it('pairs with nothing when the confirmation is cancelled', async () => {
    const { called } = renderDialog({ join_over_bluetooth: () => ({ ...synced, paired: true }) });
    fireEvent.click(await screen.findByRole('button', { name: 'Join with an invitation' }));
    fireEvent.input(screen.getByLabelText('Or type its code'), { target: { value: CODE } });

    fireEvent.click(await screen.findByRole('button', { name: 'Cancel' }));

    await waitFor(() =>
      expect(screen.queryByRole('dialog', { name: 'Pair with this device?' })).toBeNull(),
    );
    expect(called('join_over_bluetooth')).toHaveLength(0);
  });

  it('reads the invitation from a QR code image and offers a join file', async () => {
    vi.mocked(scanQrInWorker).mockResolvedValue({ status: 'found', text: QR_TEXT });
    const { called } = renderDialog({
      pick_qr_image: () => btoa('png bytes'),
      export_join_file: () => true,
    });
    fireEvent.click(await screen.findByRole('button', { name: 'Join with an invitation' }));

    fireEvent.click(screen.getByRole('button', { name: 'Open image' }));

    // Reading the QR code asks at once, without another tap.
    const confirmation = await screen.findByRole('dialog', { name: 'Pair with this device?' });
    fireEvent.click(within(confirmation).getByRole('button', { name: 'Save a join file instead' }));

    expect(await screen.findByText(/Join file saved/)).toBeTruthy();
    expect(called('export_join_file')[0].args).toEqual({ invitation: QR_TEXT });
    // What the QR code says is no business of the screen.
    expect(screen.getByLabelText<HTMLInputElement>('Or type its code').value).toBe('');
  });

  it('says why joining failed', async () => {
    renderDialog({
      join_over_bluetooth: () => {
        throw appError(
          'InvitingDeviceNotFound',
          'the device showing this invitation was not found nearby',
        );
      },
    });
    fireEvent.click(await screen.findByRole('button', { name: 'Join with an invitation' }));
    fireEvent.input(screen.getByLabelText('Or type its code'), { target: { value: CODE } });

    fireEvent.click(await screen.findByRole('button', { name: 'Pair over Bluetooth' }));

    expect((await screen.findByRole('alert')).textContent).toMatch(/was not found nearby/);
  });
});
