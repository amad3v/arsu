import { createSignal, onCleanup, onMount } from 'solid-js';

import { SegmentTabs } from '@cpt/segment-tabs';
import { isTextEntry } from '@cpt/text-entry';

import { createAddFromImage } from './add-from-image';
import { FromImage } from './from-image';
import { FromLink } from './from-link';
import { ManualEntryForm } from './manual-entry-form';
import { pasteAction } from './paste';

import type { Component } from 'solid-js';

export interface AddEntryFormProps {
  onAdded: (id: string) => void;
}

type AddMethod = 'link' | 'image' | 'manual';

/**
 * The add-entry dialog's content. It lives only while the dialog is open, so
 * everything typed here, the secret included, is dropped when it closes.
 */
export const AddEntryForm: Component<AddEntryFormProps> = (props) => {
  const [method, setMethod] = createSignal<AddMethod>('link');
  const [link, setLink] = createSignal('');
  const image = createAddFromImage((id) => props.onAdded(id));
  let linkField: HTMLTextAreaElement | undefined;

  // A paste anywhere in the dialog, on any tab: a screenshot of a QR code is
  // scanned, and an otpauth:// link goes to the link field.
  onMount(() => {
    const routePaste = (event: ClipboardEvent) => {
      const action = pasteAction(
        {
          files: Array.from(event.clipboardData?.files ?? []),
          text: event.clipboardData?.getData('text/plain') ?? '',
        },
        isTextEntry(event.target),
      );
      if (action === null) return;

      event.preventDefault();
      if (action.kind === 'image') {
        setMethod('image');
        void image.add(action.image);
      } else {
        setMethod('link');
        setLink(action.link);
        linkField?.focus();
      }
    };

    // WebKit, the engine Tauri uses on Linux, runs a keyboard paste over
    // content that is not editable (a focused tab or button) only when a
    // `beforepaste` listener cancels that event; without it, no `paste`
    // event fires at all.
    const allowPaste = (event: Event) => {
      if (!isTextEntry(event.target)) event.preventDefault();
    };

    document.addEventListener('paste', routePaste);
    document.addEventListener('beforepaste', allowPaste);
    onCleanup(() => {
      document.removeEventListener('paste', routePaste);
      document.removeEventListener('beforepaste', allowPaste);
    });
  });

  return (
    <SegmentTabs
      label={'How to add the account'}
      value={method()}
      onValueChange={setMethod}
      tabs={[
        {
          value: 'link',
          label: 'Link',
          content: () => (
            <FromLink
              link={link()}
              onLinkChange={setLink}
              onAdded={(id) => props.onAdded(id)}
              ref={(element) => {
                linkField = element;
              }}
            />
          ),
        },
        { value: 'image', label: 'QR image', content: () => <FromImage adder={image} /> },
        {
          value: 'manual',
          label: 'Manual',
          content: () => <ManualEntryForm onAdded={(id) => props.onAdded(id)} />,
        },
      ]}
    />
  );
};
