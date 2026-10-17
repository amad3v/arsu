// The app's start-up and session plumbing: which screen to open on, the
// backend's auto-lock event, the right-click menu, pinch zoom, and errors
// for the diagnostic log.

import { deviceCheck, isUnlocked, onVaultLocked, reportUiError, vaultExists } from '@api';
import { errorMessage, isAppError, isAppErrorPayload } from '@api/lib';
import { APP_NAME } from '@cpt/app-name';
import { listenWhileMounted } from '@cpt/listen';
import { isTextEntry } from '@cpt/text-entry';
import { toaster } from '@cpt/toaster';

import type { UiError } from '@app-types/api';

/** The screens of a session. */
export type Screen = 'starting' | 'rooted' | 'create' | 'unlock' | 'unlocked';

/** The screens that show the vault: to create, to unlock, or unlocked. */
export type VaultScreen = Exclude<Screen, 'starting' | 'rooted'>;

/** The screen for a vault that does or doesn't exist, and is or isn't unlocked. */
export function vaultScreen(exists: boolean, unlocked: boolean): VaultScreen {
  if (!exists) return 'create';
  return unlocked ? 'unlocked' : 'unlock';
}

/** Asks the backend which screen to open on. */
export async function detectVaultScreen(): Promise<VaultScreen> {
  const exists = await vaultExists();
  return vaultScreen(exists, exists && (await isUnlocked()));
}

/** Whether the phone is rooted and the user hasn't yet accepted the risk of it. */
export async function needsRootConsent(): Promise<boolean> {
  const device = await deviceCheck();
  return device.rooted && !device.accepted;
}

/** What the user sees when the vault can't be opened at start-up. */
export interface StartFailure {
  title: string;
  message: string;
}

export function startFailure(err: unknown): StartFailure {
  return {
    title: isAppError(err, 'VaultInUse')
      ? `${APP_NAME} is already running`
      : `${APP_NAME} couldn't open the vault`,
    message: errorMessage(err),
  };
}

/**
 * Calls `onLocked` each time the backend auto-locks the vault, for as long as
 * the calling component lives.
 */
export function onBackendLock(onLocked: () => void): void {
  listenWhileMounted(
    () => onVaultLocked(onLocked),
    (err) => {
      // The vault still locks; the app only learns of it at its next request.
      toaster.create({
        type: 'error',
        title: "The app won't notice when the vault locks itself",
        description: errorMessage(err),
      });
    },
  );
}

/**
 * Turns off WebKitGTK's own right-click menu outside text fields: it offers a
 * web page's Back, Forward and Reload, which an app has no use for (and
 * Reload would drop the UI's state). Text fields keep theirs, for cut, copy
 * and paste. Returns the function that turns it back on.
 */
export function suppressPageContextMenu(): () => void {
  const onContextMenu = (event: MouseEvent) => {
    if (!isTextEntry(event.target)) event.preventDefault();
  };
  document.addEventListener('contextmenu', onContextMenu);
  return () => {
    document.removeEventListener('contextmenu', onContextMenu);
  };
}

/**
 * Keeps a pinch on a touchscreen from zooming the app, as WebKitGTK and
 * Android's WebView otherwise do: a page that handles a two-finger touch
 * (cancels it) takes it from the engine's own gestures. One finger still
 * scrolls and taps. A touchpad's pinch never reaches the page; the shell
 * stops that one natively (`cmd::no_zoom`). Returns the function that
 * turns it back off.
 */
export function suppressPinchZoom(): () => void {
  const onTouch = (event: TouchEvent) => {
    if (event.touches.length > 1) event.preventDefault();
  };
  // Not passive: a passive listener cannot cancel the touch.
  const options = { passive: false };
  document.addEventListener('touchstart', onTouch, options);
  document.addEventListener('touchmove', onTouch, options);
  return () => {
    document.removeEventListener('touchstart', onTouch);
    document.removeEventListener('touchmove', onTouch);
  };
}

/**
 * What an uncaught `value` says, for the diagnostic log: a failed
 * command's kind and message, an `Error`'s name, message and stack, or
 * anything else as text.
 */
export function uiError(value: unknown, fatal: boolean): UiError {
  if (isAppErrorPayload(value)) {
    return { name: value.kind, message: value.message, stack: null, fatal };
  }
  if (value instanceof Error) {
    return { name: value.name, message: value.message, stack: value.stack ?? null, fatal };
  }
  return { name: null, message: errorMessage(value), stack: null, fatal };
}

function report(error: UiError) {
  reportUiError(error).catch(() => {
    // The backend is out of reach: there is nowhere left to report it.
  });
}

/** Reports the error that stopped the interface (its error screen shows). */
export function reportUiCrash(value: unknown): void {
  report(uiError(value, true));
}

/**
 * Reports every error nothing caught, thrown or rejected, to the
 * diagnostic log. Returns the function that stops.
 */
export function reportUncaughtErrors(): () => void {
  const onError = (event: ErrorEvent) => report(uiError(event.error ?? event.message, false));
  const onRejection = (event: PromiseRejectionEvent) => report(uiError(event.reason, false));
  window.addEventListener('error', onError);
  window.addEventListener('unhandledrejection', onRejection);
  return () => {
    window.removeEventListener('error', onError);
    window.removeEventListener('unhandledrejection', onRejection);
  };
}
