// Waiting for an Ark dialog to finish opening. Imported only by
// *.test.ts(x) files.

/**
 * Lets an open Ark dialog finish its setup, which it defers to the next
 * animation frames (@zag-js): its focus trap, and hiding what lies
 * behind it from assistive technology. A person is never faster than
 * that; a test is, and then sees a dialog only half set up (closing it
 * before its trap exists, or opening another inside it before the first
 * has hidden the page, whose portal it then hides too).
 */
export async function dialogSettled(): Promise<void> {
  await new Promise((resolve) => requestAnimationFrame(resolve));
  await new Promise((resolve) => requestAnimationFrame(resolve));
}
