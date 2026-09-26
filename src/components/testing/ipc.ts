// Test doubles for the backend, for component tests. Imported only by
// *.test.tsx files, so none of it reaches the app bundle.

import { mockIPC } from '@tauri-apps/api/mocks';

import type { AppErrorKind, AppErrorPayload } from '@app-types/api';
import type { InvokeArgs } from '@tauri-apps/api/core';

/** Answers one command. Throw `appError(...)` to make the command reject. */
export type CommandHandler = (args: InvokeArgs | undefined) => unknown;

export interface CommandCall {
  cmd: string;
  args: InvokeArgs | undefined;
}

/**
 * Routes every invoke() to its handler and records the call. A command with
 * no handler rejects, so an unexpected call fails the test instead of passing
 * silently.
 */
export function mockCommands(handlers: Partial<Record<string, CommandHandler>>): CommandCall[] {
  const calls: CommandCall[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    const handler = handlers[cmd];
    if (handler === undefined) throw new Error(`unexpected command: ${cmd}`);
    return handler(args);
  });
  return calls;
}

/**
 * A rejection the helpers in @api/lib recognise as the backend's
 * AppErrorPayload. It is an Error too, so it can be thrown.
 */
export function appError(kind: AppErrorKind, message: string = kind): Error & AppErrorPayload {
  return Object.assign(new Error(message), { kind });
}

/** A promise the test settles by hand, to observe a command while it runs. */
export function deferred<T>(): {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason: Error) => void;
} {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((onResolve, onReject) => {
    resolve = onResolve;
    reject = onReject;
  });
  return { promise, resolve, reject };
}
