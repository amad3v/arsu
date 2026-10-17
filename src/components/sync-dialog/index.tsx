import {
  createEffect,
  createSignal,
  createUniqueId,
  For,
  Match,
  on,
  onCleanup,
  onMount,
  Show,
  Switch,
} from 'solid-js';

import {
  exportJoinFile,
  exportSyncFile,
  findBluetoothDevices,
  getSyncOverview,
  importSyncFile,
  joinOverBluetooth,
  onSyncSession,
  pickQrImage,
  removeSyncDevice,
  startBluetoothPairing,
  startBluetoothSync,
  stopBluetooth,
  syncOverBluetooth,
} from '@api';
import { errorMessage, svgToDataUri } from '@api/lib';
import { scanQrInWorker } from '@cpt/add-entry-dialog/scan-qr';
import { IconifiedButton } from '@cpt/iconified-btn';
import { listenWhileMounted } from '@cpt/listen';
import { Modal } from '@cpt/modal';
import { QrScanner } from '@cpt/qr-scanner';
import { isTouchUi } from '@cpt/touch-ui';

import { describeSummary, isPending } from './summary';

import type {
  NearbyDevice,
  SyncDevice,
  SyncInvitation,
  SyncOverview,
  SyncSessionReport,
  SyncSummary,
} from '@app-types/api';
import type { Component, JSX } from 'solid-js';

/**
 * Read once. The touch interface scans QR codes with the camera; a phone
 * otherwise syncs as a computer does, finding devices or waiting for
 * them.
 */
const TOUCH_UI = isTouchUi();

/** What an invitation's QR code text starts with (vault_sync::invitation). */
const QR_PREFIX = 'ARSU-INVITE:';

/**
 * How many characters a typed invitation code has, dashes and spaces
 * aside (vault_sync::invitation's `CODE_CHARS`).
 */
const CODE_CHARS = 55;

/** Whether `text` is a whole typed code, though not necessarily a right one. */
const isWholeCode = (text: string) => text.replace(/[\s-]/g, '').length === CODE_CHARS;

type View = 'devices' | 'invite' | 'join' | 'nearby' | 'serve';

export interface SyncDialogProps {
  open: boolean;
  onClose: () => void;
  /** Called after a sync or pairing changed this vault. */
  onSynced: () => void;
  finalFocusEl?: () => HTMLElement | null;
}

/**
 * Sync with the vault's other devices: the devices, pairing
 * a new one, and syncing over Bluetooth or through a file. Its content
 * unmounts when the dialog closes, which stops anything it started over
 * Bluetooth.
 */
export const SyncDialog: Component<SyncDialogProps> = (props) => (
  <Modal
    open={props.open}
    title={'Sync'}
    description={'Keep this vault the same on your devices, without any network.'}
    wide
    page
    onClose={() => props.onClose()}
    finalFocusEl={props.finalFocusEl}
  >
    <Show when={props.open}>
      <SyncPanel onSynced={() => props.onSynced()} />
    </Show>
  </Modal>
);

/** The desktop header's Sync button, and the dialog it opens. */
export const SyncDialogButton: Component<{ onSynced: () => void }> = (props) => {
  const [open, setOpen] = createSignal(false);
  let trigger: HTMLButtonElement | undefined;

  return (
    <>
      <IconifiedButton
        ref={(element: HTMLButtonElement) => {
          trigger = element;
        }}
        class={'btn'}
        icon={'i-ph-arrows-clockwise'}
        label={'Sync'}
        onClick={() => setOpen(true)}
      />
      <SyncDialog
        open={open()}
        onClose={() => setOpen(false)}
        onSynced={() => props.onSynced()}
        finalFocusEl={() => trigger ?? null}
      />
    </>
  );
};

/**
 * The dialog's content. It owns every effect — fetching, Bluetooth, files —
 * as event handlers; the views below only show what it gives them.
 */
const SyncPanel: Component<{ onSynced: () => void }> = (props) => {
  const [overview, setOverview] = createSignal<SyncOverview | null>(null);
  const [view, setView] = createSignal<View>('devices');
  const [invitation, setInvitation] = createSignal<SyncInvitation | null>(null);
  const [nearby, setNearby] = createSignal<NearbyDevice[] | null>(null);
  const [report, setReport] = createSignal<SyncSessionReport | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [status, setStatus] = createSignal<string | null>(null);
  const [error, setError] = createSignal<string | null>(null);
  /** An invitation read in the join view, waiting for the user to confirm. */
  const [joining, setJoining] = createSignal<string | null>(null);
  let bluetoothRunning = false;
  /** Counts the views shown: an action that ends after its view was left is stale. */
  let shown = 0;
  /** Counts Bluetooth starts: only the latest may stop what it started. */
  let starts = 0;

  /**
   * Runs one action: one at a time, its error shown. Resolves to its
   * result, or to null if it failed.
   */
  async function attempt<T>(action: () => Promise<T>): Promise<{ value: T } | null> {
    setStatus(null);
    setError(null);
    setBusy(true);
    try {
      return { value: await action() };
    } catch (err) {
      setError(errorMessage(err));
      return null;
    } finally {
      setBusy(false);
    }
  }

  /**
   * Refreshes the device list, in the background: unlike an action, it
   * leaves the result of the last one on screen.
   */
  async function load() {
    try {
      setOverview(await getSyncOverview());
    } catch (err) {
      setError(errorMessage(err));
    }
  }

  function synced(summary: SyncSummary) {
    setStatus(describeSummary(summary));
    props.onSynced();
    void load();
  }

  function stopBluetoothIfRunning() {
    if (bluetoothRunning) {
      bluetoothRunning = false;
      void stopBluetooth();
    }
  }

  function leave() {
    stopBluetoothIfRunning();
    shown += 1;
  }

  /** Shows `next`; returns its number, to tell whether it is still shown. */
  function show(next: View): number {
    leave();
    setStatus(null);
    setError(null);
    setView(next);
    return shown;
  }

  /**
   * Starts Bluetooth for the view numbered `view`. Started after the view
   * was left, it is stopped at once, unless a later start replaced it.
   */
  async function startBluetooth<T>(view: number, start: () => Promise<T>): Promise<T | null> {
    const mine = ++starts;
    const started = await attempt(start);
    if (started === null) return null;
    if (view !== shown) {
      if (mine === starts) void stopBluetooth();
      return null;
    }
    bluetoothRunning = true;
    return started.value;
  }

  async function openInvite() {
    const view = show('invite');
    setInvitation(null);
    const opened = await startBluetooth(view, startBluetoothPairing);
    if (opened !== null) setInvitation(opened);
  }

  async function openServe() {
    await startBluetooth(show('serve'), startBluetoothSync);
  }

  async function searchNearby() {
    const view = show('nearby');
    setNearby(null);
    const found = await attempt(findBluetoothDevices);
    // Left while searching: nothing to show, nor to sync with.
    if (view !== shown) return;
    // A device showing an invitation is waiting to pair, not to sync.
    const waiting = found === null ? [] : found.value.filter((device) => !device.pairing);
    setNearby(waiting);
    // One device waiting is the one the user means: no list to tap.
    if (waiting.length === 1) await syncWith(waiting[0].address);
  }

  async function syncWith(address: string) {
    const summary = await attempt(() => syncOverBluetooth(address));
    if (summary !== null) synced(summary.value);
  }

  async function join(text: string) {
    setJoining(null);
    const summary = await attempt(() => joinOverBluetooth(text));
    if (summary !== null) synced(summary.value);
  }

  async function saveJoinFile(text: string) {
    setJoining(null);
    const saved = await attempt(() => exportJoinFile(text));
    if (saved?.value === true) {
      setStatus(
        'Join file saved. Open it on the device that showed the invitation, then open here the sync file it sends back.',
      );
    }
  }

  async function saveSyncFile(device: SyncDevice) {
    const saved = await attempt(() => exportSyncFile(device.id));
    if (saved?.value === true) {
      setStatus(`Sync file saved. Open it on ${device.name} with “Open a sync file”.`);
    }
  }

  async function remove(device: SyncDevice) {
    if ((await attempt(() => removeSyncDevice(device.id))) !== null) await load();
  }

  async function openSyncFile() {
    const summary = await attempt(importSyncFile);
    if (summary !== null && summary.value !== null) synced(summary.value);
  }

  onMount(() => void load());
  listenWhileMounted(
    () => onSyncSession(setReport),
    (err) => setError(errorMessage(err)),
  );
  // A session that ran in the background, over Bluetooth.
  createEffect(
    on(report, (current) => {
      if (current === null) return;
      if (current.kind === 'finished') synced(current.summary);
      else setError(current.error.message);
    }),
  );
  onCleanup(leave);

  return (
    <div class={'flex flex-col gap-4'}>
      <Switch>
        <Match when={view() === 'devices'}>
          <DevicesView
            overview={overview()}
            busy={busy()}
            onInvite={() => void openInvite()}
            onJoin={() => show('join')}
            onNearby={() => void searchNearby()}
            onServe={() => void openServe()}
            onSaveFile={(device) => void saveSyncFile(device)}
            onRemove={(device) => void remove(device)}
            onOpenFile={() => void openSyncFile()}
          />
        </Match>
        <Match when={view() === 'invite'}>
          <InviteView invitation={invitation()} onBack={() => show('devices')} />
        </Match>
        <Match when={view() === 'join'}>
          <JoinView
            busy={busy()}
            onRead={setJoining}
            onError={setError}
            onBack={() => show('devices')}
          />
          <JoinConfirmation
            invitation={joining()}
            onPair={(text) => void join(text)}
            onSaveJoinFile={(text) => void saveJoinFile(text)}
            onClose={() => setJoining(null)}
          />
        </Match>
        <Match when={view() === 'nearby'}>
          <NearbyView
            devices={nearby()}
            busy={busy()}
            onSync={(address) => void syncWith(address)}
            onSearchAgain={() => void searchNearby()}
            onBack={() => show('devices')}
          />
        </Match>
        <Match when={view() === 'serve'}>
          <ServeView onBack={() => show('devices')} />
        </Match>
      </Switch>

      <Show when={status()}>
        {(message) => (
          <p class={'success'} role={'status'}>
            {message()}
          </p>
        )}
      </Show>
      <Show
        when={TOUCH_UI}
        fallback={
          <Show when={error()}>
            {(message) => (
              <p class={'error'} role={'alert'}>
                {message()}
              </p>
            )}
          </Show>
        }
      >
        <ErrorSheet message={error()} onClose={() => setError(null)} />
      </Show>
    </div>
  );
};

/** On a phone, what went wrong rises in a sheet, where the eye and the thumb are. */
const ErrorSheet: Component<{ message: string | null; onClose: () => void }> = (props) => {
  // Kept while the sheet slides away, so its text doesn't vanish first.
  const [shown, setShown] = createSignal('');
  createEffect(() => {
    const message = props.message;
    if (message !== null) setShown(message);
  });

  return (
    <Modal open={props.message !== null} title={'That didn’t work'} onClose={() => props.onClose()}>
      <div class={'flex flex-col gap-4'}>
        <p role={'alert'}>{shown()}</p>
        <button type={'button'} class={'btn-primary self-end'} onClick={() => props.onClose()}>
          {'OK'}
        </button>
      </div>
    </Modal>
  );
};

// ---- the device list ---------------------------------------------------------

const DevicesView: Component<{
  overview: SyncOverview | null;
  busy: boolean;
  onInvite: () => void;
  onJoin: () => void;
  onNearby: () => void;
  onServe: () => void;
  onSaveFile: (device: SyncDevice) => void;
  onRemove: (device: SyncDevice) => void;
  onOpenFile: () => void;
}> = (props) => {
  const [removing, setRemoving] = createSignal<string | null>(null);
  /** Syncing needs a paired device; pairing and files do not. */
  const paired = () => (props.overview?.devices.length ?? 0) > 0;
  const unpairedReason = createUniqueId();
  /**
   * What `device` shares its name with, if anything: reinstalling Arsu
   * makes a new device, and leaves the old one listed under the same
   * name. Only the user can tell which is which.
   */
  const sameName = (device: SyncDevice, overview: SyncOverview) => {
    if (device.name === overview.thisDevice.name) return 'this device';
    const twin = overview.devices.some(
      (other) => other.id !== device.id && other.name === device.name,
    );
    return twin ? 'another device' : null;
  };

  return (
    <>
      <Show when={props.overview}>
        {(overview) => (
          <>
            <section class={'flex flex-col gap-2'}>
              <h3 class={'text-sm text-text-muted font-semibold'}>{'This device'}</h3>
              <DeviceCard device={overview().thisDevice} />
            </section>

            <section class={'flex flex-col gap-2'}>
              <h3 class={'text-sm text-text-muted font-semibold'}>{'Your other devices'}</h3>
              <Show
                when={overview().devices.length > 0}
                fallback={
                  <p class={'subtle'}>
                    {
                      'No other device yet. Pair one to start syncing. Each paired device then gets a Sync file button here, to sync without Bluetooth.'
                    }
                  </p>
                }
              >
                <ul class={'flex flex-col gap-2'}>
                  <For each={overview().devices}>
                    {(device) => (
                      <li>
                        <DeviceCard device={device} sameNameAs={sameName(device, overview())}>
                          <Show
                            when={removing() === device.id}
                            fallback={
                              <div class={'flex gap-2'}>
                                <button
                                  type={'button'}
                                  class={'btn'}
                                  disabled={props.busy}
                                  aria-label={`Save a sync file for ${device.name}`}
                                  onClick={() => props.onSaveFile(device)}
                                >
                                  <i class={'i-ph-file-arrow-down size-4'} aria-hidden={'true'} />
                                  {'Sync file'}
                                </button>
                                <button
                                  type={'button'}
                                  class={'btn'}
                                  disabled={props.busy}
                                  aria-label={`Remove ${device.name}`}
                                  onClick={() => setRemoving(device.id)}
                                >
                                  <i class={'i-ph-trash size-4'} aria-hidden={'true'} />
                                </button>
                              </div>
                            }
                          >
                            <div class={'flex flex-wrap gap-2 items-center'}>
                              <p class={'text-sm warning'}>
                                {'It keeps the accounts it already has.'}
                              </p>
                              <button
                                type={'button'}
                                class={'btn-danger'}
                                aria-label={`Remove ${device.name} for good`}
                                onClick={() => {
                                  setRemoving(null);
                                  props.onRemove(device);
                                }}
                              >
                                {'Remove'}
                              </button>
                              <button
                                type={'button'}
                                class={'btn-ghost'}
                                onClick={() => setRemoving(null)}
                              >
                                {'Keep'}
                              </button>
                            </div>
                          </Show>
                        </DeviceCard>
                      </li>
                    )}
                  </For>
                </ul>
              </Show>
            </section>
          </>
        )}
      </Show>

      <section class={'gap-2 grid sm:grid-cols-2'}>
        <Action
          icon={'i-ph-qr-code'}
          label={'Pair a new device'}
          onSelect={() => props.onInvite()}
        />
        <Action
          icon={'i-ph-sign-in'}
          label={'Join with an invitation'}
          onSelect={() => props.onJoin()}
        />
        <Action
          icon={'i-ph-bluetooth'}
          label={'Sync with a device nearby'}
          disabled={!paired()}
          describedBy={paired() ? undefined : unpairedReason}
          onSelect={() => props.onNearby()}
        />
        <Action
          icon={'i-ph-broadcast'}
          label={'Wait for my devices'}
          disabled={!paired()}
          describedBy={paired() ? undefined : unpairedReason}
          onSelect={() => props.onServe()}
        />
        <Action
          icon={'i-ph-file-arrow-up'}
          label={'Open a sync file'}
          disabled={props.busy}
          onSelect={() => props.onOpenFile()}
        />
      </section>
      <Show when={props.overview && !paired()}>
        <p id={unpairedReason} class={'subtle'}>
          {
            'Pair a device first to sync with it: “Pair a new device” on one device, then “Join with an invitation” on the other.'
          }
        </p>
      </Show>
    </>
  );
};

const DeviceCard: Component<{
  device: SyncDevice;
  /** What else is listed under this device's name. */
  sameNameAs?: string | null;
  children?: JSX.Element;
}> = (props) => (
  <div class={'card p-3 flex flex-wrap gap-3 items-center justify-between'}>
    <div class={'flex flex-col min-w-0'}>
      <span class={'font-medium truncate'}>{props.device.name}</span>
      <span class={'text-xs text-text-muted font-mono'}>{props.device.fingerprint}</span>
      <Show when={isPending(props.device.addedAt)}>
        <span class={'text-xs text-text-muted'}>{'Waiting for its reply'}</span>
      </Show>
      <Show when={props.sameNameAs}>
        {(other) => (
          <span class={'text-xs warning'}>
            {`Same name as ${other()}. Paired on ${new Date(props.device.addedAt * 1000).toLocaleDateString()}: if this is an earlier installation (reinstalling Arsu makes a new device), remove it.`}
          </span>
        )}
      </Show>
    </div>
    {props.children}
  </div>
);

const Action: Component<{
  icon: string;
  label: string;
  disabled?: boolean;
  /** Id of the text that explains the action, e.g. why it is disabled. */
  describedBy?: string;
  onSelect: () => void;
}> = (props) => (
  <button
    type={'button'}
    class={'btn justify-start'}
    disabled={props.disabled}
    aria-describedby={props.describedBy}
    onClick={() => props.onSelect()}
  >
    <i class={`${props.icon} size-5`} aria-hidden={'true'} />
    {props.label}
  </button>
);

const BackButton: Component<{ onBack: () => void }> = (props) => (
  <button type={'button'} class={'btn-ghost self-start'} onClick={() => props.onBack()}>
    <i class={'i-ph-arrow-left size-4'} aria-hidden={'true'} />
    {'Back'}
  </button>
);

// ---- pairing -------------------------------------------------------------------

const InviteView: Component<{
  invitation: SyncInvitation | null;
  onBack: () => void;
}> = (props) => {
  return (
    <>
      <BackButton onBack={props.onBack} />
      <Show when={props.invitation} fallback={<p class={'subtle'}>{'Opening an invitation…'}</p>}>
        {(shown) => (
          <div class={'flex flex-col gap-3 items-center'}>
            <img
              src={svgToDataUri(shown().qrSvg)}
              alt={'Invitation QR code'}
              class={'p-2 border border-border rounded-lg bg-white size-64'}
            />
            <code class={'text-sm tracking-wide font-mono text-center select-all break-all'}>
              {shown().code}
            </code>
            <p class={'subtle text-center'}>
              {
                'On the other device, choose “Join with an invitation”, then scan this code (a computer types it). Keep this screen open until it has paired.'
              }
            </p>
            <p class={'text-xs text-text-muted'}>
              {`Valid once, until ${new Date(shown().expiresAt * 1000).toLocaleTimeString([], {
                hour: '2-digit',
                minute: '2-digit',
              })}. Anyone who sees it can pair until then.`}
            </p>
          </div>
        )}
      </Show>
    </>
  );
};

const JoinView: Component<{
  busy: boolean;
  /** An invitation was read: ask before pairing with it. */
  onRead: (invitation: string) => void;
  onError: (message: string) => void;
  onBack: () => void;
}> = (props) => {
  const [reading, setReading] = createSignal(false);
  const [scanning, setScanning] = createSignal(false);
  const scans = new AbortController();
  onCleanup(() => scans.abort());

  /** Reads the invitation from `image`, and asks to pair with it. */
  async function scan(image: Blob) {
    const result = await scanQrInWorker(image, scans.signal);
    if (result.status === 'found') {
      props.onRead(result.text);
    } else if (result.status === 'not-found') {
      props.onError('There is no QR code in that image.');
    } else {
      props.onError('That image could not be read.');
    }
  }

  async function read(image: () => Promise<Blob | null>) {
    setReading(true);
    try {
      const picked = await image();
      if (picked !== null) await scan(picked);
    } catch (err) {
      if (!scans.signal.aborted) props.onError(errorMessage(err));
    } finally {
      setReading(false);
    }
  }

  function scanned(invitation: string) {
    setScanning(false);
    props.onRead(invitation);
  }

  return (
    <>
      <BackButton onBack={props.onBack} />
      <p class={'subtle text-sm'}>
        {TOUCH_UI
          ? 'Scan the QR code the other device shows, or open a picture of it.'
          : 'Open a photo or a screenshot of the QR code the other device shows, or type its code.'}
      </p>
      <div class={'flex flex-wrap gap-2'}>
        <Show when={TOUCH_UI}>
          <button
            type={'button'}
            class={'btn-primary'}
            disabled={reading() || props.busy}
            onClick={() => setScanning(true)}
          >
            <i class={'i-ph-scan size-4'} aria-hidden={'true'} />
            {'Scan QR'}
          </button>
        </Show>
        <button
          type={'button'}
          class={'btn'}
          disabled={reading() || props.busy}
          onClick={() => void read(pickQrImage)}
        >
          <i class={'i-ph-image size-4'} aria-hidden={'true'} />
          {'Open image'}
        </button>
      </div>
      {/* A computer scans no QR code: between two of them, the code is typed. */}
      <Show when={!TOUCH_UI}>
        <CodeField disabled={props.busy} onCode={props.onRead} />
      </Show>
      <Show when={props.busy}>
        <p class={'subtle'} role={'status'}>
          {'Looking for the device showing the invitation, and pairing…'}
        </p>
      </Show>
      <QrScanner
        open={scanning()}
        title={'Scan the invitation'}
        hint={'Point the camera at the QR code the other device shows.'}
        accept={(scannedText) => scannedText.startsWith(QR_PREFIX)}
        wrongCode={'That QR code is not an Arsu invitation.'}
        onScan={scanned}
        onClose={() => setScanning(false)}
      />
    </>
  );
};

/**
 * Where an invitation code is typed. It is read as soon as it is whole,
 * and again on Enter (after a cancelled confirmation, say).
 */
const CodeField: Component<{
  disabled: boolean;
  onCode: (code: string) => void;
}> = (props) => {
  const [code, setCode] = createSignal('');
  const whole = () => isWholeCode(code());
  createEffect(
    on(whole, (isWhole) => {
      if (isWhole) props.onCode(code().trim());
    }),
  );

  return (
    <div class={'flex flex-col gap-1'}>
      <label class={'label'} for={'sync-invitation-code'}>
        {'Or type its code'}
      </label>
      <input
        id={'sync-invitation-code'}
        class={'input font-mono'}
        autocomplete={'off'}
        spellcheck={false}
        placeholder={'XXXXX-XXXXX-…'}
        disabled={props.disabled}
        value={code()}
        onInput={(event) => setCode(event.currentTarget.value)}
        onKeyDown={(event) => {
          if (event.key === 'Enter' && whole()) props.onCode(code().trim());
        }}
      />
    </div>
  );
};

/**
 * Asks before pairing with the device whose invitation was read: over
 * Bluetooth, or, from its QR code, with a join file, for when Bluetooth
 * cannot reach it (a phone that cannot wait, or none nearby).
 */
const JoinConfirmation: Component<{
  invitation: string | null;
  onPair: (invitation: string) => void;
  onSaveJoinFile: (invitation: string) => void;
  onClose: () => void;
}> = (props) => {
  const [cancel, setCancel] = createSignal<HTMLButtonElement>();
  // Kept while the dialog closes, so its buttons don't change under the eye.
  const [shown, setShown] = createSignal('');
  createEffect(() => {
    const invitation = props.invitation;
    if (invitation !== null) setShown(invitation);
  });
  const fromQrCode = () => shown().startsWith(QR_PREFIX);
  /** Runs `action` once: the first tap closes the dialog, a second does nothing. */
  const once = (action: (invitation: string) => void) => () => {
    if (props.invitation !== null) action(props.invitation);
  };

  return (
    <Modal
      open={props.invitation !== null}
      title={'Pair with this device?'}
      description={
        'Arsu finds the device showing this invitation nearby and pairs with it over Bluetooth. Both then have the accounts of both vaults.'
      }
      onClose={() => props.onClose()}
      initialFocusEl={() => cancel() ?? null}
    >
      <div class={'flex flex-col gap-3'}>
        <button type={'button'} class={'btn-primary'} onClick={once(props.onPair)}>
          <i class={'i-ph-bluetooth size-4'} aria-hidden={'true'} />
          {'Pair over Bluetooth'}
        </button>
        <Show
          when={fromQrCode()}
          fallback={
            <p class={'subtle text-sm'}>
              {
                'If Bluetooth cannot reach that device, a join file can instead, and a join file needs its QR code: scan it, or open a picture of it.'
              }
            </p>
          }
        >
          <button type={'button'} class={'btn'} onClick={once(props.onSaveJoinFile)}>
            <i class={'i-ph-file-arrow-down size-4'} aria-hidden={'true'} />
            {'Save a join file instead'}
          </button>
          <p class={'subtle text-sm'}>
            {
              'For when Bluetooth cannot reach that device: open the file on it, then open here the sync file it gives back.'
            }
          </p>
        </Show>
        <button type={'button'} class={'btn-ghost'} ref={setCancel} onClick={() => props.onClose()}>
          {'Cancel'}
        </button>
      </div>
    </Modal>
  );
};

// ---- Bluetooth -----------------------------------------------------------------

const NearbyView: Component<{
  devices: NearbyDevice[] | null;
  busy: boolean;
  onSync: (address: string) => void;
  onSearchAgain: () => void;
  onBack: () => void;
}> = (props) => {
  return (
    <>
      <BackButton onBack={props.onBack} />
      <Show when={props.devices} fallback={<p class={'subtle'}>{'Looking for your devices…'}</p>}>
        {(found) => (
          <Show
            when={found().length > 0}
            fallback={
              <>
                <p class={'subtle'}>
                  {
                    'No device found nearby. On the other device, open Sync and choose “Wait for my devices”.'
                  }
                </p>
                {/* Android 8–11 tie Bluetooth scans to location: with
                    Location off, a scan finds nothing. */}
                <Show when={TOUCH_UI}>
                  <p class={'subtle'}>
                    {'On Android 11 and earlier, Location must be on for this phone to find it.'}
                  </p>
                </Show>
              </>
            }
          >
            <p class={'subtle text-sm'}>
              {
                'Only devices paired with your vault can sync. To add one, choose “Pair a new device” on it.'
              }
            </p>
            <ul class={'flex flex-col gap-2'}>
              <For each={found()}>
                {(device) => (
                  <li>
                    <button
                      type={'button'}
                      class={'btn w-full justify-start'}
                      disabled={props.busy}
                      aria-label={`Sync with ${device.address}`}
                      onClick={() => props.onSync(device.address)}
                    >
                      <i class={'i-ph-bluetooth size-5'} aria-hidden={'true'} />
                      <span class={'font-mono'}>{device.address}</span>
                    </button>
                  </li>
                )}
              </For>
            </ul>
          </Show>
        )}
      </Show>
      <button
        type={'button'}
        class={'btn self-start'}
        disabled={props.busy}
        onClick={() => props.onSearchAgain()}
      >
        <i class={'i-ph-arrow-clockwise size-4'} aria-hidden={'true'} />
        {'Search again'}
      </button>
    </>
  );
};

const ServeView: Component<{ onBack: () => void }> = (props) => {
  return (
    <>
      <BackButton onBack={props.onBack} />
      <p class={'subtle'}>
        {
          'Waiting for your devices. On another device, choose “Sync with a device nearby”. Keep this screen open.'
        }
      </p>
    </>
  );
};
