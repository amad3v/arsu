import {
  createMemo,
  createSignal,
  For,
  mapArray,
  Match,
  onCleanup,
  onMount,
  Switch,
} from 'solid-js';

import { copyCode, lockVault } from '@api';
import { errorMessage, isAppError } from '@api/lib';
import { AddEntryDialog } from '@cpt/add-entry-dialog';
import { AppTitle } from '@cpt/app-title';
import { DeleteEntry } from '@cpt/delete-entry';
import {
  createEntryCode,
  EntryCard,
  entryActionsId,
  entryRowId,
  liveCode,
  NEW_ENTRY_MS,
} from '@cpt/entry-card';
import { entryLabel } from '@cpt/entry-label';
import { ErrorPanel } from '@cpt/error-panel';
import { ExportEntryQrModal, ExportVaultDialog } from '@cpt/export-dialogs';
import { countLabel } from '@cpt/format';
import { IconifiedButton } from '@cpt/iconified-btn';
import { ImportDialog } from '@cpt/import-dialog';
import { scrollFade } from '@cpt/scroll-fade';
import { SettingsMenu, useSettings } from '@cpt/settings';
import { secondsLabel } from '@cpt/settings/options';
import { isTextEntry } from '@cpt/text-entry';
import { toaster } from '@cpt/toaster';

import { createClock } from './create-clock';
import { createEntryList } from './create-entry-list';
import { createFlash } from './create-flash';
import { NoEntries, NoMatches } from './empty-states';
import { visibleItems } from './entry-search';
import { currentHighlight, searchKeyAction, shortcutAction, stepHighlight } from './keyboard';
import { SearchField } from './search-field';

import type { EntrySummary } from '@app-types/api';
import type { EntryCode } from '@cpt/entry-card';
import type { Component } from 'solid-js';

export interface DashboardProps {
  /** The vault is locked, by the user or found locked: show the unlock screen. */
  onLocked: () => void;
}

/** How long a row shows "Copied". */
const COPIED_FEEDBACK_MS = 1_500;

const LIST_ID = 'entry-list';

/**
 * The unlocked vault: a searchable list of entries and their codes. The search
 * field has focus from the start, so finding and copying a code is: type, Enter.
 */
export const Dashboard: Component<DashboardProps> = (props) => {
  const settings = useSettings();
  const list = createEntryList({ onLocked: () => props.onLocked() });

  // One clock for every countdown, running only while there is a TOTP entry.
  const hasTotp = createMemo(() => list.state.entries.some((entry) => entry.otpType === 'totp'));
  const now = createClock(hasTotp);

  // One code per entry, kept while the entry is in the vault (filtered out or
  // not), so neither a search nor a refresh throws a code away.
  const codes = mapArray(
    () => list.state.entries,
    (entry) =>
      createEntryCode(entry, {
        now,
        onMissing: () => void list.refresh(),
        onLocked: () => props.onLocked(),
      }),
  );

  const [query, setQuery] = createSignal('');
  const visible = createMemo(() => visibleItems(codes(), query()));
  const visibleIds = createMemo(() => visible().map((item) => item.entry.id));
  const [chosen, setChosen] = createSignal<string | null>(null);
  const highlighted = createMemo(() => currentHighlight(visibleIds(), chosen()));
  const activeDescendant = () => {
    const id = highlighted();
    return id === null ? null : entryRowId(id);
  };
  const [searchFocused, setSearchFocused] = createSignal(false);

  const [copiedId, flashCopied] = createFlash<string>(COPIED_FEEDBACK_MS);
  const [newIds, flashNew] = createFlash<ReadonlySet<string>>(NEW_ENTRY_MS);
  const [announcement, setAnnouncement] = createSignal('');

  const [deleteTarget, setDeleteTarget] = createSignal<EntrySummary | null>(null);
  const [exportTarget, setExportTarget] = createSignal<EntrySummary | null>(null);
  // Not reactive: each holds the id of the row a dialog was opened from, read
  // only once that dialog closes (by then `deleteTarget`/`exportTarget` are
  // already cleared), to give focus back to the row's ⋮ menu.
  let deleteTargetId: string | null = null;
  let exportTargetId: string | null = null;

  const [root, setRoot] = createSignal<HTMLDivElement>();
  const [search, setSearch] = createSignal<HTMLInputElement>();

  const summary = () => {
    const total = countLabel(list.state.entries.length, 'entry', 'entries');
    return query().trim() === '' ? total : `${visible().length} of ${total}`;
  };

  function changeQuery(value: string) {
    setQuery(value);
    setChosen(null);
    if (value.trim() !== '') {
      setAnnouncement(countLabel(visible().length, 'match', 'matches'));
    }
  }

  function scrollRowIntoView(entryId: string) {
    document.getElementById(entryRowId(entryId))?.scrollIntoView({ block: 'nearest' });
  }

  /** A row dialog's return focus target: its ⋮ menu trigger, or the search field once the row is gone. */
  function rowActionsOrSearch(entryId: string | null): HTMLElement | null {
    const trigger = entryId === null ? null : document.getElementById(entryActionsId(entryId));
    return trigger ?? search() ?? null;
  }

  /** Nothing focused, or focus in the dashboard (not in a dialog or menu, which are portalled out). */
  function isInDashboard(node: Node): boolean {
    return node === document.body || root()?.contains(node) === true;
  }

  function clearSearch() {
    changeQuery('');
    search()?.focus();
  }

  function focusSearch() {
    search()?.focus();
    search()?.select();
  }

  /** Copies the entry's code; for an HOTP entry with no code shown, generates one first. */
  async function copy(item: EntryCode) {
    let code = liveCode(item.state, Date.now());
    if (code === null && item.entry.otpType === 'hotp') {
      await item.generate();
      code = item.state.code;
    }
    if (code === null) return;

    setChosen(item.entry.id);
    // Emptied first, so that copying the same entry again is announced again.
    setAnnouncement('');

    try {
      await copyCode(code);
      flashCopied(item.entry.id);
      setAnnouncement(
        `Copied the code for ${entryLabel(item.entry)}. The clipboard clears in ${secondsLabel(settings.values.clipboardClearSeconds)}.`,
      );
    } catch (err) {
      if (isAppError(err, 'Locked')) {
        props.onLocked();
        return;
      }
      setAnnouncement("The code couldn't be copied.");
      toaster.create({
        type: 'error',
        title: "The code couldn't be copied",
        description: errorMessage(err),
      });
    }
  }

  async function lock() {
    try {
      await lockVault();
      props.onLocked();
    } catch (err) {
      toaster.create({
        type: 'error',
        title: "The vault couldn't be locked",
        description: errorMessage(err),
      });
    }
  }

  /** After an add or an import: list the new entries, mark them, and bring the first into view. */
  async function showNew(ids: readonly string[]) {
    changeQuery('');
    await list.refresh();
    flashNew(new Set(ids));

    const first = visibleIds().find((id) => ids.includes(id));
    if (first !== undefined) {
      setChosen(first);
      scrollRowIntoView(first);
    }
  }

  /** The deleted row took its menu button with it: focus goes back to the search. */
  async function afterDelete() {
    await list.refresh();
    search()?.focus();
  }

  function onSearchKeyDown(event: KeyboardEvent) {
    const action = searchKeyAction(event);

    switch (action) {
      case 'next':
      case 'previous': {
        event.preventDefault();
        const id = stepHighlight(visibleIds(), highlighted(), action === 'next' ? 1 : -1);
        setChosen(id);
        if (id !== null) scrollRowIntoView(id);
        break;
      }
      case 'copy': {
        event.preventDefault();
        const item = visible().find((candidate) => candidate.entry.id === highlighted());
        if (item !== undefined) void copy(item);
        break;
      }
      case 'clear':
        if (query() !== '') {
          event.preventDefault();
          changeQuery('');
        }
        break;
      case null:
        break;
    }
  }

  onMount(() => {
    search()?.focus();

    // Shortcuts work anywhere in the dashboard, but not in a dialog or a menu,
    // which own their keys.
    const onKeyDown = (event: KeyboardEvent) => {
      const target = event.target;
      if (!(target instanceof Node) || !isInDashboard(target)) return;

      // The empty search field isn't taking text yet: "/" there is still the
      // shortcut (a no-op, as focus is already there), not a search for "/".
      const typing = isTextEntry(target) && !(target === search() && query() === '');
      const action = shortcutAction(event, typing);
      if (action === null) return;

      event.preventDefault();
      if (action === 'focus-search') focusSearch();
      else void lock();
    };

    window.addEventListener('keydown', onKeyDown);
    onCleanup(() => {
      window.removeEventListener('keydown', onKeyDown);
    });
  });

  return (
    <div ref={setRoot} class={'mx-auto px-4 pt-4 flex flex-col gap-3 h-full max-w-3xl'}>
      <header class={'flex flex-wrap gap-x-3 gap-y-2 items-center'}>
        <AppTitle class={'mr-auto'} summary={list.state.status === 'ready' ? summary() : null} />

        <div class={'flex gap-2 items-center'}>
          <AddEntryDialog onSuccess={(id) => void showNew([id])} />
          <ImportDialog onImported={(result) => void showNew(result.importedIds)} />
          <ExportVaultDialog />
          <IconifiedButton
            class={'btn'}
            onClick={() => void lock()}
            icon={'i-ph-lock'}
            label={'Lock vault'}
            aria-keyshortcuts={'Control+L'}
          />
          <span class={'mx-1 bg-border h-6 w-px'} aria-hidden={'true'} />
          <SettingsMenu />
        </div>
      </header>

      <SearchField
        ref={setSearch}
        value={query()}
        onValueChange={changeQuery}
        onClear={clearSearch}
        onKeyDown={onSearchKeyDown}
        onFocusChange={setSearchFocused}
        controls={LIST_ID}
        activeDescendant={activeDescendant()}
      />

      {/* The only region that scrolls. */}
      <div ref={scrollFade} class={'scroll-fade pb-4 flex-1 min-h-0 overflow-y-auto'}>
        <Switch>
          <Match when={list.state.status === 'loading'}>
            <p class={'appear-delayed subtle'}>{'Loading entries…'}</p>
          </Match>
          <Match when={list.state.status === 'failed'}>
            <ErrorPanel
              title={"The entries couldn't be loaded"}
              message={list.state.error ?? ''}
              onRetry={() => void list.refresh()}
            />
          </Match>
          <Match when={list.state.entries.length === 0}>
            <NoEntries />
          </Match>
          <Match when={visible().length === 0}>
            <NoMatches query={query()} onClear={clearSearch} />
          </Match>
        </Switch>

        <ul
          id={LIST_ID}
          role={'grid'}
          aria-label={'Entries'}
          hidden={visible().length === 0}
          class={
            'border border-border rounded-xl bg-bg-card shadow-sm overflow-hidden divide-border divide-y'
          }
        >
          <For each={visible()}>
            {(item) => (
              <EntryCard
                code={item}
                now={now}
                highlighted={searchFocused() && highlighted() === item.entry.id}
                copied={copiedId() === item.entry.id}
                isNew={newIds()?.has(item.entry.id) ?? false}
                onSelect={() => {
                  setChosen(item.entry.id);
                  search()?.focus();
                }}
                onCopy={() => void copy(item)}
                onExport={() => {
                  exportTargetId = item.entry.id;
                  setExportTarget(item.entry);
                }}
                onDelete={() => {
                  deleteTargetId = item.entry.id;
                  setDeleteTarget(item.entry);
                }}
              />
            )}
          </For>
        </ul>
      </div>

      <p class={'sr-only'} role={'status'}>
        {announcement()}
      </p>

      <DeleteEntry
        entry={deleteTarget()}
        onClose={() => setDeleteTarget(null)}
        onDeleted={() => {
          setDeleteTarget(null);
          void afterDelete();
        }}
        finalFocusEl={() => rowActionsOrSearch(deleteTargetId)}
      />

      <ExportEntryQrModal
        entry={exportTarget()}
        onClose={() => setExportTarget(null)}
        finalFocusEl={() => rowActionsOrSearch(exportTargetId)}
        onMissing={() => {
          setExportTarget(null);
          void list.refresh();
        }}
      />
    </div>
  );
};
