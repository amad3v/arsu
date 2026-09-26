import type { Component } from 'solid-js';

/** The first run: no entries yet, and the two ways to add them. */
export const NoEntries: Component = () => (
  <div class={'card py-10 text-center flex flex-col gap-2 items-center'}>
    <i class={'i-ph-key text-text-muted size-8'} aria-hidden={'true'} />
    <h2 class={'font-semibold'}>{'No entries yet'}</h2>
    <p class={'subtle max-w-md'}>
      {'Use '}
      <strong class={'text-text font-medium'}>{'Add entry'}</strong>
      {
        ' in the toolbar to scan a QR code, paste an otpauth:// link or type a secret. Moving from Aegis or 2FAS? '
      }
      <strong class={'text-text font-medium'}>{'Import entries'}</strong>
      {' brings over a backup file.'}
    </p>
  </div>
);

export interface NoMatchesProps {
  query: string;
  onClear: () => void;
}

/** A search that matches nothing. */
export const NoMatches: Component<NoMatchesProps> = (props) => (
  <div class={'py-10 text-center flex flex-col gap-2 items-center'}>
    <h2 class={'font-semibold'}>{`No entries match “${props.query.trim()}”`}</h2>
    <p class={'subtle'}>{'Search looks at issuers and account names.'}</p>
    <button type={'button'} class={'btn mt-1'} onClick={() => props.onClear()}>
      {'Clear search'}
    </button>
  </div>
);
