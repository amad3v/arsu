import { createSignal, Match, onCleanup, onMount, Show, Switch } from 'solid-js';

import { Modal } from '@cpt/modal';

import {
  cameraNamed,
  cameraOf,
  cameraProblem,
  camerasToTry,
  cameraStopped,
  openCamera,
  rememberCamera,
  rememberedCamera,
  stopCamera,
} from './camera';
import { createFrameReader } from './frame-reader';

import type { CameraProblem } from './camera';
import type { Component } from 'solid-js';

/** How long to wait between two frames: a few reads a second, without heating the phone. */
export const FRAME_INTERVAL_MS = 150;

/** How often to check that the camera still runs. */
export const CAMERA_CHECK_MS = 250;

/** How long a camera shows pictures before it is remembered as one that works. */
export const CAMERA_PROVEN_MS = 2000;

/**
 * How long to wait for the system to list its cameras again after one has
 * stopped: a failed camera service takes some seconds to come back.
 */
export const CAMERA_RETURN_MS = 15_000;

/** How often to look for the cameras while waiting for them. */
export const CAMERA_LIST_MS = 500;

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

/** Said while the scanner moves on from a camera that stopped to another. */
const CAMERA_SWITCH_TEXT = 'The camera stopped. Starting another one…';

const Viewfinder: Component<{
  hint: string;
  accept: (text: string) => boolean;
  wrongCode: string;
  onScan: (text: string) => void;
  onCancel: () => void;
}> = (props) => {
  const [problem, setProblem] = createSignal<CameraProblem | null>(null);
  const [switching, setSwitching] = createSignal(false);
  const [wrong, setWrong] = createSignal(false);
  const [video, setVideo] = createSignal<HTMLVideoElement>();
  const reader = createFrameReader();
  /** Cameras that stopped or would not open since the scanner opened. */
  const failed = new Set<string>();
  let stream: MediaStream | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let watch: ReturnType<typeof setInterval> | undefined;
  /** This launch's id of the camera remembered here, once looked up. */
  let rememberedId: string | undefined;
  /** Whether the scanner chose the camera in use, rather than the system. */
  let chosen = false;
  /** When the camera in use showed its first picture. */
  let firstPicture: number | undefined;
  let proven = false;
  let gone = false;

  /** Turns the camera in use off and stops reading and watching it. */
  function dropCamera() {
    clearTimeout(timer);
    clearInterval(watch);
    if (stream !== null) stopCamera(stream);
    stream = null;
  }

  function letGo() {
    gone = true;
    dropCamera();
    reader.close();
  }

  /**
   * Reads the frame on screen, then the next one after a pause, for as long
   * as `from` is the camera in use. Only `play` and its own timer call it,
   * and dropping the camera clears the timer.
   */
  async function scan(preview: HTMLVideoElement, from: MediaStream) {
    // Until the camera sends its first picture, there is nothing to read.
    if (preview.readyState >= HTMLMediaElement.HAVE_CURRENT_DATA && preview.videoWidth > 0) {
      notePicture(from);
      const result = await reader.read(await createImageBitmap(preview));
      // Closed, or the camera changed, while the frame was read.
      if (gone || stream !== from) return;
      if (result.status === 'found') {
        if (props.accept(result.text)) {
          letGo();
          props.onScan(result.text);
          return;
        }
        setWrong(true);
      }
    }
    timer = setTimeout(() => void scan(preview, from), FRAME_INTERVAL_MS);
  }

  /**
   * Remembers a camera the scanner chose once it has shown pictures for a
   * while. The system's own choice, when it works, needs no remembering.
   */
  function notePicture(from: MediaStream) {
    const now = performance.now();
    firstPicture ??= now;
    if (!proven && now - firstPicture >= CAMERA_PROVEN_MS) {
      proven = true;
      const { name } = cameraOf(from);
      if (chosen && name !== '') rememberCamera(name);
    }
  }

  /**
   * Starts the camera remembered here if the system still lists it, or else
   * the system's rear camera.
   */
  async function begin() {
    const name = rememberedCamera();
    if (name === undefined) {
      void start();
      return;
    }
    try {
      rememberedId = await cameraNamed(name);
    } catch (err) {
      if (!gone) setProblem(cameraProblem(err));
      return;
    }
    if (gone) return;
    // Gone from this device: the system chooses again.
    if (rememberedId === undefined) rememberCamera(undefined);
    void start(rememberedId);
  }

  /** Turns a camera on and starts reading it: `deviceId`, or else the system's rear camera. */
  async function start(deviceId?: string) {
    let opened: MediaStream;
    try {
      opened = await openCamera(deviceId);
    } catch (err) {
      if (gone) return;
      const why = cameraProblem(err);
      if (why === 'denied' || deviceId === undefined) {
        setProblem(why);
        return;
      }
      // A chosen camera that would not open: another may.
      failed.add(deviceId);
      if (deviceId === rememberedId) rememberCamera(undefined);
      void switchCamera();
      return;
    }
    // Closed while Android asked for the permission.
    const preview = video();
    if (gone || preview === undefined) {
      stopCamera(opened);
      return;
    }
    stream = opened;
    chosen = deviceId !== undefined;
    firstPicture = undefined;
    proven = false;
    setSwitching(false);
    preview.srcObject = opened;
    watch = setInterval(() => {
      if (stream !== null && cameraStopped(stream)) cameraLost();
    }, CAMERA_CHECK_MS);
    void play(preview, opened);
  }

  /** The camera in use stopped on its own: sets it aside and moves on to another. */
  function cameraLost() {
    if (stream === null) return;
    const lost = cameraOf(stream);
    dropCamera();
    // A camera the system does not name cannot be told from the others.
    if (lost.id === undefined) {
      setProblem('failed');
      return;
    }
    failed.add(lost.id);
    if (rememberedCamera() === lost.name) rememberCamera(undefined);
    void switchCamera();
  }

  /**
   * Starts the best camera not set aside, waiting for the system to list its
   * cameras again if it lists none.
   */
  async function switchCamera() {
    setSwitching(true);
    const giveUp = performance.now() + CAMERA_RETURN_MS;
    for (;;) {
      const cameras = await camerasToTry(failed);
      if (gone) return;
      if (cameras.length > 0) {
        void start(cameras[0]);
        return;
      }
      if (performance.now() >= giveUp) {
        setProblem('failed');
        return;
      }
      await new Promise((waited) => setTimeout(waited, CAMERA_LIST_MS));
    }
  }

  /** Starts the preview, then reads it, unless closed or the camera changed meanwhile. */
  async function play(preview: HTMLVideoElement, from: MediaStream) {
    try {
      await preview.play();
    } catch (err) {
      if (gone || stream !== from) return;
      if (cameraStopped(from)) {
        cameraLost();
        return;
      }
      letGo();
      setProblem(cameraProblem(err));
      return;
    }
    if (!gone && stream === from) void scan(preview, from);
  }

  onMount(() => void begin());
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
            {switching() ? CAMERA_SWITCH_TEXT : wrong() ? props.wrongCode : props.hint}
          </p>
        </Match>
      </Switch>
      <button type={'button'} class={'btn self-center'} onClick={() => props.onCancel()}>
        {'Cancel'}
      </button>
    </div>
  );
};
