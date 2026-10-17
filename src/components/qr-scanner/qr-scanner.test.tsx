import { render, screen, waitFor } from '@solidjs/testing-library';
import { createSignal } from 'solid-js';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { createFrameReader } from './frame-reader';

import { QrScanner } from '.';

import type { QrScan } from '@cpt/add-entry-dialog/qr-image';

vi.mock('./frame-reader', () => ({ createFrameReader: vi.fn() }));

const INVITATION = 'ARSU-INVITE:0123456789';

/** A camera stream whose one track records being stopped. */
function fakeStream() {
  const track = { stop: vi.fn() };
  return { track, stream: { getTracks: () => [track] } as unknown as MediaStream };
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
  return { onScan, onClose, close: () => setOpen(false) };
}

let getUserMedia: ReturnType<typeof vi.fn>;

beforeEach(() => {
  getUserMedia = vi.fn();
  // jsdom has no camera API.
  Object.defineProperty(navigator, 'mediaDevices', {
    value: { getUserMedia },
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
