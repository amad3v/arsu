import type { AppErrorKind, AppErrorPayload } from '@app-types/api';

/**
 * True if `err` is the structured error every command rejects with: an object
 * with a string `kind` and a string `message`. Tauri's own rejections (bad
 * argument types, a command the capability doesn't grant) are plain strings.
 */
export function isAppErrorPayload(err: unknown): err is AppErrorPayload {
  return (
    typeof err === 'object' &&
    err !== null &&
    'kind' in err &&
    typeof err.kind === 'string' &&
    'message' in err &&
    typeof err.message === 'string'
  );
}

/** True if `err` is an AppErrorPayload of that kind. The only way to branch on an error. */
export function isAppError<K extends AppErrorKind>(
  err: unknown,
  kind: K,
): err is AppErrorPayload & { kind: K } {
  return isAppErrorPayload(err) && err.kind === kind;
}

/** The text to show for any rejection or thrown value. Display it; never parse it. */
export function errorMessage(err: unknown): string {
  if (isAppErrorPayload(err)) return err.message;
  if (err instanceof Error) return err.message;
  return String(err);
}

/**
 * An `<img src>` for an SVG document. Rendering through `<img>` keeps any
 * markup in the SVG inert, unlike `innerHTML`.
 */
export function svgToDataUri(svg: string): string {
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

/** The bytes of a base64 string, as the backend sends binary data. */
export function base64ToBytes(base64: string): Uint8Array<ArrayBuffer> {
  const binary = atob(base64);
  return Uint8Array.from(binary, (char) => char.charCodeAt(0));
}
