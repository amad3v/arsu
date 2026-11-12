import { render, screen, waitFor } from '@solidjs/testing-library';
import { createSignal } from 'solid-js';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { camerasToTry } from './camera';
import { createFrameReader } from './frame-reader';

import { QrScanner } from '.';

import type { QrScan } from '@cpt/add-entry-dialog/qr-image';

vi.mock('./frame-reader', () => ({ createFrameReader: vi.fn() }));

const INVITATION = 'ARSU-INVITE:0123456789';

/** A camera stream whose one track records being stopped, and can stop on its own. */
function fakeStream(deviceId = 'main') {
  const track = {
    readyState: 'live' as MediaStreamTrackState,
    label: deviceId,
    stop: vi.fn(),
    getSettings: () => ({ deviceId }),
  };
  const stream = { getTracks: () => [track], getVideoTracks: () => [track] };
  return { track, stream: stream as unknown as MediaStream };
}

/** A camera as the system lists it. */
function camera(deviceId: string, facing: 'environment' | 'user', width: number) {
  return {
    kind: 'videoinput',
    deviceId,
    label: deviceId,
    getCapabilities: () => ({ facingMode: [facing], width: { min: 1, max: width } }),
  };
}

/** The constraints of each camera asked for, in order. */
function camerasAskedFor() {
  return getUserMedia.mock.calls.map(
    ([constraints]) => (constraints as MediaStreamConstraints).video as MediaTrackConstraints,
  );
}

/** The frame reader answers each frame with the next scan, then finds nothing. */
function readerAnswering(...scans: QrScan[]) {
  const close = vi.fn();
  const read = vi.fn(() => Promise.resolve(scans.shift() ?? { status: 'not-found' as const }));
  vi.mocked(createFrameReader).mockReturnValue({ read, close });
  return { read, close };
}

function renderScanner(onScan = vi.fn()) {
  const [open, setOpen] = createSignal(true);
  const onClose = vi.fn(() => setOpen(false));
  render(() => (
    <QrScanner
      open={open()}
      title={'Scan the invitation'}
      hint={'Point the camera at the QR code.'}
      accept={(text) => text.startsWith('ARSU-INVITE:')}
      wrongCode={'That QR code is not an Arsu invitation.'}
      onScan={onScan}
      onClose={onClose}
    />
  ));
  return { onScan, onClose, close: () => setOpen(false), reopen: () => setOpen(true) };
}

let getUserMedia: ReturnType<typeof vi.fn>;
let enumerateDevices: ReturnType<typeof vi.fn>;

beforeEach(() => {
  localStorage.clear();
  getUserMedia = vi.fn();
  enumerateDevices = vi.fn(() => Promise.resolve([]));
  // jsdom has no camera API.
  Object.defineProperty(navigator, 'mediaDevices', {
    value: { getUserMedia, enumerateDevices },
    configurable: true,
  });
  vi.stubGlobal(
    'createImageBitmap',
    vi.fn(() => Promise.resolve({ width: 640, height: 480, close: vi.fn() })),
  );
  // jsdom plays no media: the video is ready at once.
  vi.spyOn(HTMLMediaElement.prototype, 'play').mockResolvedValue();
  vi.spyOn(HTMLMediaElement.prototype, 'readyState', 'get').mockReturnValue(4);
  vi.spyOn(HTMLVideoElement.prototype, 'videoWidth', 'get').mockReturnValue(640);
});

afterEach(() => {
  Reflect.deleteProperty(navigator, 'mediaDevices');
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('the QR scanner', () => {
  it('opens the rear camera, without sound', async () => {
    const { stream } = fakeStream();
    getUserMedia.mockResolvedValue(stream);
    readerAnswering();

    renderScanner();

    await waitFor(() => expect(getUserMedia).toHaveBeenCalledOnce());
    const [constraints] = getUserMedia.mock.calls[0] as [MediaStreamConstraints];
    expect(constraints.audio).toBe(false);
    expect(constraints.video).toMatchObject({ facingMode: { ideal: 'environment' } });
    expect(screen.getByText('Point the camera at the QR code.')).toBeTruthy();
  });

  it('reads frames until it finds the code, then gives it and lets the camera go', async () => {
    const { stream, track } = fakeStream();
    getUserMedia.mockResolvedValue(stream);
    const { read, close } = readerAnswering(
      { status: 'not-found' },
      { status: 'found', text: INVITATION },
    );

    const { onScan } = renderScanner();

    await waitFor(() => expect(onScan).toHaveBeenCalledWith(INVITATION));
    expect(read).toHaveBeenCalledTimes(2);
    expect(track.stop).toHaveBeenCalled();
    expect(close).toHaveBeenCalled();
  });

  it('goes on past a code of another kind, and says so', async () => {
    getUserMedia.mockResolvedValue(fakeStream().stream);
    readerAnswering(
      { status: 'found', text: 'https://example.com' },
      { status: 'not-found' },
      { status: 'found', text: INVITATION },
    );

    const { onScan } = renderScanner();

    expect(await screen.findByText('That QR code is not an Arsu invitation.')).toBeTruthy();
    await waitFor(() => expect(onScan).toHaveBeenCalledWith(INVITATION));
    expect(onScan).toHaveBeenCalledOnce();
  });

  it('says how to allow the camera when it is refused', async () => {
    getUserMedia.mockRejectedValue(new DOMException('denied', 'NotAllowedError'));
    readerAnswering();

    renderScanner();

    expect(await screen.findByText(/Allow the camera in the system settings/)).toBeTruthy();
  });

  it('says when there is no camera', async () => {
    getUserMedia.mockRejectedValue(new DOMException('none', 'NotFoundError'));
    readerAnswering();

    renderScanner();

    expect(await screen.findByText(/no camera/)).toBeTruthy();
  });

  it('lets the camera go when it closes', async () => {
    const { stream, track } = fakeStream();
    getUserMedia.mockResolvedValue(stream);
    const { close: closeReader } = readerAnswering();
    const { close } = renderScanner();
    await waitFor(() => expect(getUserMedia).toHaveBeenCalledOnce());

    close();

    await waitFor(() => expect(track.stop).toHaveBeenCalled());
    expect(closeReader).toHaveBeenCalled();
  });

  it('reads no frame once closed while the preview started', async () => {
    getUserMedia.mockResolvedValue(fakeStream().stream);
    let playing: () => void = () => undefined;
    const play = vi.spyOn(HTMLMediaElement.prototype, 'play').mockReturnValue(
      new Promise<void>((resolve) => {
        playing = resolve;
      }),
    );
    const { read } = readerAnswering();
    const { close } = renderScanner();
    await waitFor(() => expect(play).toHaveBeenCalled());

    close();
    playing();
    await new Promise((settled) => setTimeout(settled, 300));

    expect(read).not.toHaveBeenCalled();
  });

  it('says the camera failed when its preview cannot start, and lets it go', async () => {
    const { stream, track } = fakeStream();
    getUserMedia.mockResolvedValue(stream);
    vi.spyOn(HTMLMediaElement.prototype, 'play').mockRejectedValue(
      new DOMException('no autoplay', 'NotAllowedError'),
    );
    readerAnswering();
    renderScanner();

    await waitFor(() => expect(track.stop).toHaveBeenCalled());
  });

  it('lets go of a camera that opens only after it has closed', async () => {
    const { stream, track } = fakeStream();
    let grant: (stream: MediaStream) => void = () => undefined;
    getUserMedia.mockReturnValue(
      new Promise<MediaStream>((resolve) => {
        grant = resolve;
      }),
    );
    readerAnswering();
    const { close } = renderScanner();
    await waitFor(() => expect(getUserMedia).toHaveBeenCalledOnce());

    close();
    grant(stream);

    await waitFor(() => expect(track.stop).toHaveBeenCalled());
  });
});

describe('a camera that stops', () => {
  const CAMERA_KEY = 'arsu.scanner-camera';

  it('is let go, and the next rear camera starts', async () => {
    const broken = fakeStream('broken');
    getUserMedia.mockResolvedValueOnce(broken.stream).mockResolvedValue(fakeStream('main').stream);
    enumerateDevices.mockResolvedValue([
      camera('broken', 'environment', 4608),
      camera('main', 'environment', 4608),
    ]);
    readerAnswering();
    renderScanner();
    await waitFor(() => expect(getUserMedia).toHaveBeenCalledOnce());

    broken.track.readyState = 'ended';

    expect(await screen.findByText('The camera stopped. Starting another one…')).toBeTruthy();
    await waitFor(() => expect(getUserMedia).toHaveBeenCalledTimes(2));
    expect(broken.track.stop).toHaveBeenCalled();
    expect(camerasAskedFor()[1]).toMatchObject({ deviceId: { exact: 'main' } });
    expect(await screen.findByText('Point the camera at the QR code.')).toBeTruthy();
  });

  it('waits for the system to list its cameras again', async () => {
    const broken = fakeStream('broken');
    getUserMedia.mockResolvedValueOnce(broken.stream).mockResolvedValue(fakeStream('main').stream);
    // A failed camera service lists no camera until it is back.
    enumerateDevices
      .mockResolvedValueOnce([])
      .mockResolvedValue([
        camera('broken', 'environment', 4608),
        camera('main', 'environment', 4608),
      ]);
    readerAnswering();
    renderScanner();
    await waitFor(() => expect(getUserMedia).toHaveBeenCalledOnce());

    broken.track.readyState = 'ended';

    await waitFor(() => expect(getUserMedia).toHaveBeenCalledTimes(2), { timeout: 3000 });
    expect(camerasAskedFor()[1]).toMatchObject({ deviceId: { exact: 'main' } });
  });

  it('says the camera failed once no other camera is left', async () => {
    const broken = fakeStream('broken');
    getUserMedia.mockResolvedValue(broken.stream);
    enumerateDevices.mockResolvedValue([camera('broken', 'environment', 4608)]);
    readerAnswering();
    renderScanner();
    await waitFor(() => expect(getUserMedia).toHaveBeenCalledOnce());
    // Time runs fast: the wait for other cameras is over at its first look.
    let clock = 0;
    vi.spyOn(performance, 'now').mockImplementation(() => (clock += 60_000));

    broken.track.readyState = 'ended';

    expect(await screen.findByText(/The camera could not start/)).toBeTruthy();
    expect(getUserMedia).toHaveBeenCalledOnce();
  });

  it('is forgotten when it was the remembered one', async () => {
    localStorage.setItem(CAMERA_KEY, 'broken');
    const broken = fakeStream('broken');
    getUserMedia.mockResolvedValueOnce(broken.stream).mockResolvedValue(fakeStream('main').stream);
    enumerateDevices.mockResolvedValue([
      camera('broken', 'environment', 4608),
      camera('main', 'environment', 4608),
    ]);
    readerAnswering();
    renderScanner();
    await waitFor(() => expect(getUserMedia).toHaveBeenCalledOnce());
    expect(camerasAskedFor()[0]).toMatchObject({ deviceId: { exact: 'broken' } });

    broken.track.readyState = 'ended';

    await waitFor(() => expect(getUserMedia).toHaveBeenCalledTimes(2));
    expect(localStorage.getItem(CAMERA_KEY)).toBeNull();
  });
});

describe('the camera the scanner moved to', () => {
  const CAMERA_KEY = 'arsu.scanner-camera';
  /** Time runs a second per look at the clock, so a camera proves itself at once. */
  function fastClock() {
    let clock = 0;
    vi.spyOn(performance, 'now').mockImplementation(() => (clock += 1000));
  }

  it('is remembered by name once it keeps working, and opened first next time', async () => {
    fastClock();
    const broken = fakeStream('broken');
    getUserMedia.mockResolvedValueOnce(broken.stream).mockResolvedValue(fakeStream('main').stream);
    enumerateDevices.mockResolvedValue([
      camera('broken', 'environment', 4608),
      camera('main', 'environment', 4608),
    ]);
    readerAnswering();
    const { close, reopen } = renderScanner();
    await waitFor(() => expect(getUserMedia).toHaveBeenCalledOnce());
    broken.track.readyState = 'ended';

    await waitFor(() => expect(localStorage.getItem(CAMERA_KEY)).toBe('main'));
    close();
    reopen();

    await waitFor(() => expect(getUserMedia).toHaveBeenCalledTimes(3));
    expect(camerasAskedFor()[2]).toMatchObject({ deviceId: { exact: 'main' } });
  });

  it("is not needed when the system's own camera works: nothing is remembered", async () => {
    fastClock();
    getUserMedia.mockResolvedValue(fakeStream('main').stream);
    const { read } = readerAnswering();
    renderScanner();

    await waitFor(() => expect(read.mock.calls.length).toBeGreaterThan(3));
    expect(localStorage.getItem(CAMERA_KEY)).toBeNull();
  });

  it('is found by name when the system hides its cameras until one is allowed', async () => {
    localStorage.setItem(CAMERA_KEY, 'main');
    const allowing = fakeStream('front');
    getUserMedia
      .mockResolvedValueOnce(allowing.stream)
      .mockResolvedValue(fakeStream('main').stream);
    enumerateDevices
      .mockResolvedValueOnce([{ kind: 'videoinput', deviceId: '', label: '' }])
      .mockResolvedValue([camera('front', 'user', 4608), camera('main', 'environment', 4608)]);
    readerAnswering();
    renderScanner();

    await waitFor(() => expect(getUserMedia).toHaveBeenCalledTimes(2));
    expect(allowing.track.stop).toHaveBeenCalled();
    expect(camerasAskedFor()[1]).toMatchObject({ deviceId: { exact: 'main' } });
  });

  it("gives way to the system's camera when it is no longer listed, and is forgotten", async () => {
    localStorage.setItem(CAMERA_KEY, 'unplugged');
    getUserMedia.mockResolvedValue(fakeStream('main').stream);
    enumerateDevices.mockResolvedValue([camera('main', 'environment', 4608)]);
    readerAnswering();
    renderScanner();

    await waitFor(() => expect(getUserMedia).toHaveBeenCalledOnce());
    expect(camerasAskedFor()[0]).toMatchObject({ facingMode: { ideal: 'environment' } });
    expect(localStorage.getItem(CAMERA_KEY)).toBeNull();
  });

  it('gives way to another camera when it will not open, and is forgotten', async () => {
    localStorage.setItem(CAMERA_KEY, 'busy');
    getUserMedia
      .mockRejectedValueOnce(new DOMException('in use', 'NotReadableError'))
      .mockResolvedValue(fakeStream('main').stream);
    enumerateDevices.mockResolvedValue([
      camera('busy', 'environment', 4608),
      camera('main', 'environment', 4608),
    ]);
    readerAnswering();
    renderScanner();

    await waitFor(() => expect(getUserMedia).toHaveBeenCalledTimes(2));
    expect(camerasAskedFor()[1]).toMatchObject({ deviceId: { exact: 'main' } });
    expect(localStorage.getItem(CAMERA_KEY)).toBeNull();
    expect(screen.getByText('Point the camera at the QR code.')).toBeTruthy();
  });
});

describe('the cameras to try', () => {
  it('puts rear cameras first, the sharpest first, and leaves out those that failed', async () => {
    enumerateDevices.mockResolvedValue([
      camera('front', 'user', 4608),
      camera('failed', 'environment', 4608),
      camera('second', 'environment', 2592),
      camera('main', 'environment', 4608),
      { kind: 'audioinput', deviceId: 'microphone', label: 'microphone' },
    ]);

    expect(await camerasToTry(new Set(['failed']))).toEqual(['main', 'second', 'front']);
  });

  it('keeps the system order for cameras that say nothing of themselves', async () => {
    enumerateDevices.mockResolvedValue([
      { kind: 'videoinput', deviceId: 'first', label: '' },
      { kind: 'videoinput', deviceId: '', label: '' },
      { kind: 'videoinput', deviceId: 'second', label: '' },
    ]);

    expect(await camerasToTry(new Set())).toEqual(['first', 'second']);
  });
});
