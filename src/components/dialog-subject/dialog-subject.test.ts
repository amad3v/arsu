import { createRoot, createSignal } from 'solid-js';
import { describe, expect, it } from 'vitest';

import { createDialogSubject } from '.';

describe('createDialogSubject', () => {
  it('is null until the dialog is opened for something', () => {
    createRoot((dispose) => {
      const [target] = createSignal<string | null>(null);

      expect(createDialogSubject(target)()).toBeNull();
      dispose();
    });
  });

  it('follows the subject and keeps the last one after it is cleared', () => {
    createRoot((dispose) => {
      const [target, setTarget] = createSignal<string | null>('GitHub');
      const subject = createDialogSubject(target);

      expect(subject()).toBe('GitHub');

      setTarget(null);
      expect(subject()).toBe('GitHub');

      setTarget('GitLab');
      expect(subject()).toBe('GitLab');
      dispose();
    });
  });
});
