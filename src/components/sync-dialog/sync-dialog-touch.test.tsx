import { fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library';
import { emit } from '@tauri-apps/api/event';
import { Show } from 'solid-js';
import { describe, expect, it, vi } from 'vitest';

import { appError, mockCommands } from '@cpt/testing/ipc';

import { SyncDialog } from '.';

import type { CommandHandler } from '@cpt/testing/ipc';

// A phone: it finds and joins devices, and waits for them as a computer does.
vi.mock('@cpt/touch-ui', () => ({ isTouchUi: () => true }));
// The camera is the scanner's own business (see its tests): here it reads
// the code at a tap.
vi.mock('@cpt/qr-scanner', () => ({
  QrScanner: (props: { open: boolean; title: string; onScan: (text: string) => void }) => (
    <Show when={props.open}>
      <button type={'button'} onClick={() => props.onScan('ARSU-INVITE:0123456789')}>
        {`${props.title}: fake scan`}
      </button>
    </Show>
  ),
}));

const thisDevice = { id: 'aa'.repeat(32), name: 'Pixel', fingerprint: 'AAAA', addedAt: 1 };
const computer = { id: 'bb'.repeat(32), name: 'Computer', fingerprint: 'BBBB', addedAt: 2 };
const invitation = { qrSvg: '<svg/>', code: 'ABCDE-FGHJK', expiresAt: 1_760_000_900 };

/**
 * The page's commands, with events, and stopping Bluetooth answered: the
 * page stops it as it unmounts, after each test.
 */
function mockSync(handlers: Partial<Record<string, CommandHandler>>) {
  return mockCommands({ stop_bluetooth: () => null, ...handlers }, { events: true });
}

/** A button once it can be pressed: the sync actions wait for the device list. */
async function ready(name: string): Promise<HTMLButtonElement> {
  const button = await screen.findByRole<HTMLButtonElement>('button', { name });
  await waitFor(() => {
    expect(button.disabled).toBe(false);
  });
  return button;
}

describe('the sync page on a phone', () => {
  it('waits for devices as a computer does', async () => {
    const calls = mockSync({
      get_sync_overview: () => ({ thisDevice, devices: [computer] }),
      start_bluetooth_sync: () => null,
    });
    render(() => <SyncDialog open onClose={vi.fn()} onSynced={vi.fn()} />);

    fireEvent.click(await ready('Wait for my devices'));

    await waitFor(() =>
      expect(calls.filter((call) => call.cmd === 'start_bluetooth_sync')).toHaveLength(1),
    );
    expect(await screen.findByText(/Waiting for your devices/)).toBeTruthy();
  });

  it('says to pair first while no device is paired', async () => {
    mockSync({ get_sync_overview: () => ({ thisDevice, devices: [] }) });
    render(() => <SyncDialog open onClose={vi.fn()} onSynced={vi.fn()} />);

    const nearby = await screen.findByRole<HTMLButtonElement>('button', {
      name: 'Sync with a device nearby',
      description: /Pair a device first/,
    });
    expect(nearby.disabled).toBe(true);
  });

  it('shows an invitation and waits for the device that reads it', async () => {
    const calls = mockSync({
      get_sync_overview: () => ({ thisDevice, devices: [] }),
      start_bluetooth_pairing: () => invitation,
      stop_bluetooth: () => null,
    });
    render(() => <SyncDialog open onClose={vi.fn()} onSynced={vi.fn()} />);

    fireEvent.click(await screen.findByRole('button', { name: 'Pair a new device' }));

    expect(await screen.findByText('ABCDE-FGHJK')).toBeTruthy();
    expect(calls.map((call) => call.cmd)).toEqual(['get_sync_overview', 'start_bluetooth_pairing']);
    fireEvent.click(screen.getByRole('button', { name: 'Back' }));
    await waitFor(() =>
      expect(calls.filter((call) => call.cmd === 'stop_bluetooth')).toHaveLength(1),
    );
  });

  it('reminds that older Android versions scan only with Location on', async () => {
    mockSync({
      get_sync_overview: () => ({ thisDevice, devices: [computer] }),
      find_bluetooth_devices: () => [],
    });
    render(() => <SyncDialog open onClose={vi.fn()} onSynced={vi.fn()} />);

    fireEvent.click(await ready('Sync with a device nearby'));

    expect(await screen.findByText(/No device found nearby/)).toBeTruthy();
    expect(screen.getByText(/Android 11 and earlier/)).toBeTruthy();
  });

  it('says when this phone cannot wait, leaving the join file', async () => {
    mockSync({
      get_sync_overview: () => ({ thisDevice, devices: [] }),
      start_bluetooth_pairing: () => invitation,
    });
    render(() => <SyncDialog open onClose={vi.fn()} onSynced={vi.fn()} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Pair a new device' }));
    expect(await screen.findByText('ABCDE-FGHJK')).toBeTruthy();

    await emit('sync-session', {
      kind: 'failed',
      error: {
        kind: 'BluetoothCannotWait',
        message: 'this phone cannot wait for other devices over Bluetooth',
      },
    });

    const sheet = await screen.findByRole('dialog', {
      name: 'That didn’t work',
    });
    expect(within(sheet).getByText(/cannot wait/)).toBeTruthy();
    // The invitation stays, for a join file.
    expect(screen.getByText('ABCDE-FGHJK')).toBeTruthy();
  });

  it('scans an invitation with the camera, then asks before pairing', async () => {
    mockSync({ get_sync_overview: () => ({ thisDevice, devices: [] }) });
    render(() => <SyncDialog open onClose={vi.fn()} onSynced={vi.fn()} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Join with an invitation' }));

    fireEvent.click(screen.getByRole('button', { name: 'Scan QR' }));
    fireEvent.click(
      await screen.findByRole('button', {
        name: 'Scan the invitation: fake scan',
      }),
    );

    const confirmation = await screen.findByRole('dialog', {
      name: 'Pair with this device?',
    });
    expect(within(confirmation).getByRole('button', { name: 'Pair over Bluetooth' })).toBeTruthy();
    expect(
      within(confirmation).getByRole('button', {
        name: 'Save a join file instead',
      }),
    ).toBeTruthy();
  });

  it('joins by the QR code alone', async () => {
    mockSync({ get_sync_overview: () => ({ thisDevice, devices: [] }) });
    render(() => <SyncDialog open onClose={vi.fn()} onSynced={vi.fn()} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Join with an invitation' }));

    expect(await screen.findByRole('button', { name: 'Scan QR' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Open image' })).toBeTruthy();
    // Scanning or opening asks at once: nothing to type, nothing to confirm.
    expect(screen.queryByRole('textbox')).toBeNull();
    expect(screen.queryByRole('button', { name: 'Continue' })).toBeNull();
  });

  it('says what went wrong in a sheet', async () => {
    mockSync({
      get_sync_overview: () => ({ thisDevice, devices: [computer] }),
      find_bluetooth_devices: () => {
        throw appError('BluetoothUnavailable', 'Bluetooth is not available: it is off');
      },
    });
    render(() => <SyncDialog open onClose={vi.fn()} onSynced={vi.fn()} />);

    fireEvent.click(await ready('Sync with a device nearby'));

    const sheet = await screen.findByRole('dialog', {
      name: 'That didn’t work',
    });
    expect(within(sheet).getByText(/it is off/)).toBeTruthy();
    fireEvent.click(within(sheet).getByRole('button', { name: 'OK' }));
    await waitFor(() =>
      expect(screen.queryByRole('dialog', { name: 'That didn’t work' })).toBeNull(),
    );
  });
});
