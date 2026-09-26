import { mergeProps, splitProps } from 'solid-js';

import type { BaseButtonProps } from '@app-types/ui';
import type { Component } from 'solid-js';

/**
 * A button whose only content is an icon. `label` is its accessible name, so
 * an icon-only button can never be announced as just "button".
 */
export const BaseButton: Component<BaseButtonProps> = (props) => {
  // Defaults first: mergeProps is last-wins, so the caller's props override them.
  const merged = mergeProps({ iconSz: 'size-5', type: 'button' } as const, props);
  const [local, buttonProps] = splitProps(merged, ['icon', 'iconSz', 'label']);

  return (
    <button {...buttonProps} aria-label={local.label}>
      <i class={[local.icon, local.iconSz].join(' ')} aria-hidden={'true'} />
    </button>
  );
};
