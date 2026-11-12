// The phone's rear camera, as the QR scanner uses it. The WebView asks for
// Android's camera permission the first time (Tauri's wry turns the page's
// request into the runtime prompt), and the scanner lets the camera go as
// soon as it has read a code or closes.
//
// A phone can list several rear cameras, and one of them can fail in the
// phone's own camera service: it opens, then stops after its first picture
// and takes every camera down with it for a few seconds. The WebView may
// pick that one for "the rear camera". So the scanner watches the camera it
// got, moves on to another when it stops, and remembers the one it moved to,
// to open that one first next time.

/** Why the camera could not start. */
export type CameraProblem = 'denied' | 'missing' | 'failed';

/** A resolution a QR code reads well at, without making each frame slow to read. */
const RESOLUTION = { width: { ideal: 1280 }, height: { ideal: 720 } };

/**
 * A camera's video, without sound: the device `deviceId`, or else the rear
 * camera the system picks.
 */
export function openCamera(deviceId?: string): Promise<MediaStream> {
  return navigator.mediaDevices.getUserMedia({
    audio: false,
    video:
      deviceId === undefined
        ? { facingMode: { ideal: 'environment' }, ...RESOLUTION }
        : { deviceId: { exact: deviceId }, ...RESOLUTION },
  });
}

/** What a failed `getUserMedia` means for the user. */
export function cameraProblem(err: unknown): CameraProblem {
  if (!(err instanceof DOMException)) return 'failed';
  switch (err.name) {
    case 'NotAllowedError':
    case 'SecurityError':
      return 'denied';
    case 'NotFoundError':
    case 'OverconstrainedError':
      return 'missing';
    default:
      return 'failed';
  }
}

/** Turns the camera off. */
export function stopCamera(stream: MediaStream): void {
  for (const track of stream.getTracks()) track.stop();
}

/** A camera a stream comes from: its id for this launch, and its lasting name. */
export interface CameraInUse {
  id: string | undefined;
  name: string;
}

/** Which camera a stream comes from, as far as the system says. */
export function cameraOf(stream: MediaStream): CameraInUse {
  const [track] = stream.getVideoTracks() as (MediaStreamTrack | undefined)[];
  const id = track?.getSettings().deviceId;
  // An empty id names no camera.
  return { id: id === '' ? undefined : id, name: track?.label ?? '' };
}

/**
 * Whether a camera the scanner still holds has stopped on its own. Polled:
 * Android's WebView ends the track when the camera service fails, without
 * firing `ended`.
 */
export function cameraStopped(stream: MediaStream): boolean {
  return stream.getVideoTracks().some((track) => track.readyState === 'ended');
}

/** The cameras the system lists, ids and names hidden until a camera is allowed. */
async function listCameras(): Promise<MediaDeviceInfo[]> {
  return (await navigator.mediaDevices.enumerateDevices()).filter(
    (device) => device.kind === 'videoinput',
  );
}

/**
 * The cameras to try, best first, leaving out `failed`: rear cameras before
 * the others, and among them the sharpest, which is the phone's main camera
 * as a rule. Empty while the system lists no camera, as it does for a few
 * seconds after its camera service has failed.
 */
export async function camerasToTry(failed: ReadonlySet<string>): Promise<string[]> {
  const rank = (await listCameras())
    .filter((device) => device.deviceId !== '' && !failed.has(device.deviceId))
    .map((device, order) => {
      const caps = 'getCapabilities' in device ? (device as InputDeviceInfo).getCapabilities() : {};
      return {
        id: device.deviceId,
        rear: caps.facingMode?.includes('environment') ?? false,
        width: caps.width?.max ?? 0,
        order,
      };
    });
  rank.sort((a, b) => Number(b.rear) - Number(a.rear) || b.width - a.width || a.order - b.order);
  return rank.map((camera) => camera.id);
}

/**
 * This launch's id of the camera called `name`, if the system lists it. The
 * system shows camera ids and names only once a camera has been allowed, so
 * when they are hidden, its default camera is opened and closed first: the
 * way the web asks for the camera before choosing one.
 */
export async function cameraNamed(name: string): Promise<string | undefined> {
  let cameras = await listCameras();
  if (cameras.every((device) => device.deviceId === '')) {
    stopCamera(await navigator.mediaDevices.getUserMedia({ audio: false, video: true }));
    cameras = await listCameras();
  }
  return cameras.find((device) => device.deviceId !== '' && device.label === name)?.deviceId;
}

const CAMERA_KEY = 'arsu.scanner-camera';

/**
 * The name of the camera the scanner moved to when the system's own choice
 * stopped, if remembered on this device. A name, as the system gives camera
 * ids anew at each launch.
 */
export function rememberedCamera(): string | undefined {
  try {
    return localStorage.getItem(CAMERA_KEY) ?? undefined;
  } catch {
    return undefined;
  }
}

/** Remembers the camera to open first next time; `undefined` forgets it. */
export function rememberCamera(name: string | undefined): void {
  try {
    if (name === undefined) localStorage.removeItem(CAMERA_KEY);
    else localStorage.setItem(CAMERA_KEY, name);
  } catch {
    // Not remembered: the next scan starts with the system's rear camera.
  }
}
