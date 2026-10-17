// The phone's rear camera, as the QR scanner uses it. The WebView asks for
// Android's camera permission the first time (Tauri's wry turns the page's
// request into the runtime prompt), and the scanner lets the camera go as
// soon as it has read a code or closes.

/** Why the camera could not start. */
export type CameraProblem = 'denied' | 'missing' | 'failed';

/**
 * The rear camera's video, without sound. Asks for a resolution a QR code
 * reads well at without making each frame slow to read.
 */
export function openRearCamera(): Promise<MediaStream> {
  return navigator.mediaDevices.getUserMedia({
    audio: false,
    video: {
      facingMode: { ideal: 'environment' },
      width: { ideal: 1280 },
      height: { ideal: 720 },
    },
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
