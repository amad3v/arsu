import { describe, expect, it } from 'vitest';

import { isTouchUi } from '.';

describe('isTouchUi', () => {
  it('is on for Android WebViews, and off for the Linux desktop', () => {
    expect(
      isTouchUi(
        'Mozilla/5.0 (Linux; Android 14; Pixel 7 Build/UQ1A; wv) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/140.0 Mobile Safari/537.36',
      ),
    ).toBe(true);
    expect(
      isTouchUi(
        'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15',
      ),
    ).toBe(false);
  });
});
