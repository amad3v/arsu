import { createSignal, Match, onCleanup, onMount, Show, Switch } from 'solid-js';

import { Modal } from '@cpt/modal';

import { cameraProblem, openRearCamera, stopCamera } from './camera';
import { createFrameReader } from './frame-reader';

import type { CameraProblem } from './camera';
import type { Component } from 'solid-js';

/** How long to wait between two frames: a few reads a second, without heating the phone. */
export const FRAME_INTERVAL_MS = 150;

export interface QrScannerProps {
  open: boolean;
  title: string;
  /** What to point the camera at. */
  hint: string;
  /** Whether a code is the kind wanted here; others are passed over. */
  accept: (text: string) => boolean;
  /** Said while the camera sees a code of another kind. */
  wrongCode: string;
  /** Called with the first wanted code; the camera is off by then. */
  onScan: (text: string) => void;
  onClose: () => void;
}

/**
 * A live QR scanner: the rear camera's picture, read a few times a second
 * until it shows a code of the wanted kind. A full-screen page on a phone.
 * The camera is on only while it is open (its content mounts when it opens
 * and unmounts when it closes), and it asks for the camera permission the
 * first time, never before.
 */
export const QrScanner: Component<QrScannerProps> = (props) => (
  <Modal open={props.open} title={props.title} page onClose={() => props.onClose()}>
    <Show when={props.open}>
      <Viewfinder
        hint={props.hint}
        accept={props.accept}
        wrongCode={props.wrongCode}
        onScan={props.onScan}
        onCancel={() => props.onClose()}
      />
    </Show>
  </Modal>
);

const PROBLEM_TEXT: Record<CameraProblem, string> = {
  denied:
    'Arsu may not use the camera. Allow the camera in the system settings, or open a picture of the code instead.',
  missing: 'This device has no camera to scan with. Open a picture of the code instead.',
  failed:
    'The camera could not start. Close other apps that use it, or open a picture of the code instead.',
};

const Viewfinder: Component<{
  hint: string;
  accept: (text: string) => boolean;
  wrongCode: string;
  onScan: (text: string) => void;
  onCancel: () => void;
}> = (props) => {
  const [problem, setProblem] = createSignal<CameraProblem | null>(null);
  const [wrong, setWrong] = createSignal(false);
  const [video, setVideo] = createSignal<HTMLVideoElement>();
  const reader = createFrameReader();
  let stream: MediaStream | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let gone = false;

  function letGo() {
    gone = true;
    clearTimeout(timer);
    if (stream !== null) stopCamera(stream);
    stream = null;
    reader.close();
  }

  /**
   * Reads the frame on screen, then the next one after a pause. Only `start`
   * and its own timer call it, and letting go clears the timer.
   */
  async function scan(preview: HTMLVideoElement) {
    // Until the camera sends its first picture, there is nothing to read.
    if (preview.readyState >= HTMLMediaElement.HAVE_CURRENT_DATA && preview.videoWidth > 0) {
      const result = await reader.read(await createImageBitmap(preview));
      // Closed while the frame was read.
      if (gone) return;
      if (result.status === 'found') {
        if (props.accept(result.text)) {
          letGo();
          props.onScan(result.text);
          return;
        }
        setWrong(true);
      }
    }
    timer = setTimeout(() => void scan(preview), FRAME_INTERVAL_MS);
  }

  /** Turns the camera on and starts reading it. */
  async function start() {
    let opened: MediaStream;
    try {
      opened = await openRearCamera();
    } catch (err) {
      if (!gone) setProblem(cameraProblem(err));
      return;
    }
    // Closed while Android asked for the permission.
    const preview = video();
    if (gone || preview === undefined) {
      stopCamera(opened);
      return;
    }
    stream = opened;
    preview.srcObject = opened;
    await preview.play();
    void scan(preview);
  }

  onMount(() => void start());
  onCleanup(letGo);

  return (
    <div class={'flex flex-col gap-4 h-full'}>
      <Switch>
        <Match when={problem()}>{(found) => <p role={'alert'}>{PROBLEM_TEXT[found()]}</p>}</Match>
        <Match when={problem() === null}>
          <div class={'rounded-xl bg-black flex-1 min-h-64 relative overflow-hidden'}>
            <video
              ref={setVideo}
              class={'h-full w-full inset-0 absolute object-cover'}
              autoplay
              muted
              playsinline
              aria-label={'Camera'}
            />
            {/* Where to put the code; the whole picture is read, not just the square. */}
            <div
              class={
                'border-2 border-white/80 rounded-2xl size-60 shadow-[0_0_0_100vmax_rgba(0,0,0,0.35)] left-1/2 top-1/2 absolute -translate-x-1/2 -translate-y-1/2'
              }
              aria-hidden={'true'}
            />
          </div>
          <p class={'text-center'} aria-live={'polite'}>
            {wrong() ? props.wrongCode : props.hint}
          </p>
        </Match>
      </Switch>
      <button type={'button'} class={'btn self-center'} onClick={() => props.onCancel()}>
        {'Cancel'}
      </button>
    </div>
  );
};
