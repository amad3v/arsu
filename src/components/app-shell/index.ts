// The app's start-up and session plumbing: which screen to open on, the
// backend's auto-lock event, and the right-click menu.

import { onCleanup, onMount } from 'solid-js';

import { deviceCheck, isUnlocked, onVaultLocked, vaultExists } from '@api';
import { errorMessage, isAppError } from '@api/lib';
import { APP_NAME } from '@cpt/app-name';
import { isTextEntry } from '@cpt/text-entry';
import { toaster } from '@cpt/toaster';

import type { UnlistenFn } from '@tauri-apps/api/event';

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
  onMount(() => {
    let unlisten: UnlistenFn | undefined;
    let disposed = false;

    onVaultLocked(onLocked).then(
      (stop) => {
        if (disposed) stop();
        else unlisten = stop;
      },
      (err: unknown) => {
        // The vault still locks; the app only learns of it at its next request.
        toaster.create({
          type: 'error',
          title: "The app won't notice when the vault locks itself",
          description: errorMessage(err),
        });
      },
    );

    onCleanup(() => {
      disposed = true;
      unlisten?.();
    });
  });
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
