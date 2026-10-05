// Which interface the app shows: the desktop one, built for a mouse and a
// keyboard, or the touch one, built for a phone.

/**
 * Whether to show the touch interface: on Android, whose WebView names itself
 * in its user agent. Read once, at start-up; a phone doesn't change platform.
 */
export function isTouchUi(userAgent: string = navigator.userAgent): boolean {
  return /\bAndroid\b/.test(userAgent);
}
