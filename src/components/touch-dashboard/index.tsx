import { createMemo, createSignal, For, mapArray, Match, Show, Switch } from 'solid-js';

import { copyCode, lockVault } from '@api';
import { errorMessage, isAppError } from '@api/lib';
import { AddEntryModal } from '@cpt/add-entry-dialog';
import { AppTitle } from '@cpt/app-title';
import { createBackDismiss } from '@cpt/back-dismiss';
import { createClock } from '@cpt/dashboard/create-clock';
import { createEntryList } from '@cpt/dashboard/create-entry-list';
import { createFlash } from '@cpt/dashboard/create-flash';
import { visibleItems } from '@cpt/dashboard/entry-search';
import { DeleteEntry } from '@cpt/delete-entry';
import { createEntryCode, liveCode, NEW_ENTRY_MS } from '@cpt/entry-card';
import { entryLabel } from '@cpt/entry-label';
import { ErrorPanel } from '@cpt/error-panel';
import { ExportEntryQrModal, ExportSaved, ExportVaultModal } from '@cpt/export-dialogs';
import { countLabel } from '@cpt/format';
import { ImportModal, ImportResult } from '@cpt/import-dialog';
import { useSettings } from '@cpt/settings';
import { secondsLabel } from '@cpt/settings/options';
import { Sheet, SheetAction } from '@cpt/sheet';
import { toaster } from '@cpt/toaster';

import { SettingsPage } from './settings-page';
import { TouchEntryRow } from './touch-entry-row';

import type { EntrySummary } from '@app-types/api';
import type { EntryCode } from '@cpt/entry-card';
import type { ImportReport } from '@cpt/import-dialog';
import type { Component } from 'solid-js';

export interface TouchDashboardProps {
  /** The vault is locked, by the user or found locked: show the unlock screen. */
  onLocked: () => void;
}

/** How long a row shows "Copied". */
const COPIED_FEEDBACK_MS = 1_500;

/**
 * The unlocked vault in the touch interface: the desktop dashboard's list,
 * laid out for a phone. Its dialogs are bottom sheets, each replacing the one
 * it was opened from. A tap on an entry copies its code; its ⋮ button opens
 * the rarer actions in a bottom sheet; the + button adds entries, imports
 * or exports them, and the settings are a page of their own.
 */
export const TouchDashboard: Component<TouchDashboardProps> = (props) => {
  const settings = useSettings();
  const list = createEntryList({ onLocked: () => props.onLocked() });

  const hasTotp = createMemo(() => list.state.entries.some((entry) => entry.otpType === 'totp'));
  const now = createClock(hasTotp);
  const codes = mapArray(
    () => list.state.entries,
    (entry) =>
      createEntryCode(entry, {
        now,
        onMissing: () => void list.refresh(),
        onLocked: () => props.onLocked(),
      }),
  );

  const [searching, setSearching] = createSignal(false);
  const [query, setQuery] = createSignal('');
  const visible = createMemo(() => visibleItems(codes(), query()));

  const [copiedId, flashCopied] = createFlash<string>(COPIED_FEEDBACK_MS);
  const [newIds, flashNew] = createFlash<ReadonlySet<string>>(NEW_ENTRY_MS);
  const [announcement, setAnnouncement] = createSignal('');

  const [plusOpen, setPlusOpen] = createSignal(false);
  const [addOpen, setAddOpen] = createSignal(false);
  const [importOpen, setImportOpen] = createSignal(false);
  // What the last import did, shown in a sheet over the list it changed.
  const [importReport, setImportReport] = createSignal<ImportReport | null>(null);
  const [exportOpen, setExportOpen] = createSignal(false);
  // Whether a backup was just saved, said in a sheet as an import's result is.
  const [exportSaved, setExportSaved] = createSignal(false);
  const [settingsOpen, setSettingsOpen] = createSignal(false);
  const [actionsFor, setActionsFor] = createSignal<EntryCode | null>(null);
  const [deleteTarget, setDeleteTarget] = createSignal<EntrySummary | null>(null);
  const [exportTarget, setExportTarget] = createSignal<EntrySummary | null>(null);

  // Back leaves the search before it leaves the app.
  createBackDismiss(searching, () => closeSearch(), true);

  const actionsTitle = () => {
    const item = actionsFor();
    return item === null ? '' : entryLabel(item.entry);
  };

  const summary = () => {
    const total = countLabel(list.state.entries.length, 'entry', 'entries');
    return query().trim() === '' ? total : `${visible().length} of ${total}`;
  };

  function closeSearch() {
    setSearching(false);
    setQuery('');
  }

  /** Copies the entry's code; for an HOTP entry with no code shown, generates one first. */
  async function copy(item: EntryCode) {
    let code = liveCode(item.state, Date.now());
    if (code === null && item.entry.otpType === 'hotp') {
      await item.generate();
      code = item.state.code;
    }
    if (code === null) return;
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
    closeSearch();
    await list.refresh();
    flashNew(new Set(ids));
    const first = visible().find((item) => ids.includes(item.entry.id));
    if (first !== undefined) {
      document.getElementById(rowId(first.entry.id))?.scrollIntoView({ block: 'nearest' });
    }
  }

  // What to open once the sheet or dialog on screen has slid away, so that the
  // next one (the + sheet's dialogs, an action's sheet, a result) replaces it
  // rather than rising over it.
  let next: (() => void) | null = null;

  function openNext() {
    const open = next;
    next = null;
    open?.();
  }

  /** Opens one of the + sheet's dialogs, once the sheet has closed. */
  function fromPlus(open: (value: boolean) => void) {
    setPlusOpen(false);
    next = () => open(true);
  }

  /** Runs one of the actions sheet's actions, once the sheet has closed. */
  function act(run: (item: EntryCode) => void) {
    const item = actionsFor();
    setActionsFor(null);
    if (item !== null) next = () => run(item);
  }

  return (
    <div class={'flex flex-col h-full'}>
      <header class={'px-2 flex shrink-0 gap-1 h-14 items-center'}>
        <Show
          when={searching()}
          fallback={
            <>
              <AppTitle
                class={'px-2 flex-1'}
                summary={list.state.status === 'ready' ? summary() : null}
              />
              <HeaderButton
                icon={'i-ph-magnifying-glass'}
                label={'Search entries'}
                onClick={() => setSearching(true)}
              />
              <HeaderButton icon={'i-ph-lock'} label={'Lock vault'} onClick={() => void lock()} />
              <HeaderButton
                icon={'i-ph-gear-six'}
                label={'Settings'}
                onClick={() => setSettingsOpen(true)}
              />
            </>
          }
        >
          <HeaderButton
            icon={'i-ph-arrow-left'}
            label={'Close search'}
            onClick={() => closeSearch()}
          />
          <input
            ref={(element) => queueMicrotask(() => element.focus())}
            type={'text'}
            enterkeyhint={'search'}
            class={'input text-base flex-1'}
            placeholder={'Search by issuer or account'}
            aria-label={'Search entries'}
            autocomplete={'off'}
            spellcheck={false}
            value={query()}
            onInput={(event) => setQuery(event.currentTarget.value)}
          />
          <Show when={query() !== ''}>
            <HeaderButton icon={'i-ph-x'} label={'Clear search'} onClick={() => setQuery('')} />
          </Show>
        </Show>
      </header>

      {/* The only region that scrolls; its bottom padding keeps the last row clear of the + button. */}
      <div class={'px-4 pb-24 flex-1 min-h-0 overflow-y-auto'}>
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
            <div class={'card py-10 text-center flex flex-col gap-2 items-center'}>
              <i class={'i-ph-key text-text-muted size-8'} aria-hidden={'true'} />
              <h2 class={'font-semibold'}>{'No entries yet'}</h2>
              <p class={'subtle max-w-md'}>
                {'Tap '}
                <strong class={'text-text font-medium'}>{'+'}</strong>
                {
                  ' to add one from a screenshot of its QR code, an otpauth:// link or its secret, or to import a backup from Aegis or 2FAS.'
                }
              </p>
            </div>
          </Match>
          <Match when={visible().length === 0}>
            <div class={'py-10 text-center flex flex-col gap-2 items-center'}>
              <h2 class={'font-semibold'}>{`No entries match “${query().trim()}”`}</h2>
              <p class={'subtle'}>{'Search looks at issuers and account names.'}</p>
            </div>
          </Match>
        </Switch>

        <ul
          aria-label={'Entries'}
          hidden={visible().length === 0}
          class={
            'border border-border rounded-xl bg-bg-card shadow-sm overflow-hidden divide-border divide-y'
          }
        >
          <For each={visible()}>
            {(item) => (
              <TouchEntryRow
                id={rowId(item.entry.id)}
                code={item}
                now={now}
                copied={copiedId() === item.entry.id}
                isNew={newIds()?.has(item.entry.id) ?? false}
                onCopy={() => void copy(item)}
                onActions={() => setActionsFor(item)}
              />
            )}
          </For>
        </ul>
      </div>

      <button
        type={'button'}
        class={'btn-primary rounded-2xl size-14 shadow-lg bottom-6 right-6 fixed z-20'}
        aria-label={'Add, import or export'}
        onClick={() => setPlusOpen(true)}
      >
        <i class={'i-ph-plus-bold size-6'} aria-hidden={'true'} />
      </button>

      <p class={'sr-only'} role={'status'}>
        {announcement()}
      </p>

      <Sheet
        open={actionsFor() !== null}
        title={actionsTitle()}
        onClose={() => setActionsFor(null)}
        onExitComplete={openNext}
      >
        <Show when={actionsFor()?.entry.otpType === 'hotp'}>
          <SheetAction
            icon={'i-ph-arrow-clockwise'}
            label={'Generate the next code'}
            onSelect={() => act((item) => void item.generate())}
          />
        </Show>
        <SheetAction
          icon={'i-ph-qr-code'}
          label={'Show QR code'}
          onSelect={() => act((item) => setExportTarget(item.entry))}
        />
        <SheetAction
          icon={'i-ph-trash'}
          label={'Delete'}
          danger
          onSelect={() => act((item) => setDeleteTarget(item.entry))}
        />
      </Sheet>

      <Sheet
        open={plusOpen()}
        title={'Add or move entries'}
        onClose={() => setPlusOpen(false)}
        onExitComplete={openNext}
      >
        <SheetAction
          icon={'i-ph-plus-circle'}
          label={'Add entry'}
          description={'From a QR code screenshot, a link or its secret key'}
          onSelect={() => fromPlus(setAddOpen)}
        />
        <SheetAction
          icon={'i-ph-upload'}
          label={'Import entries'}
          description={'From an Aegis or 2FAS backup'}
          onSelect={() => fromPlus(setImportOpen)}
        />
        <SheetAction
          icon={'i-ph-download'}
          label={'Export encrypted backup'}
          description={'Keep a copy: uninstalling Arsu deletes the vault'}
          onSelect={() => fromPlus(setExportOpen)}
        />
      </Sheet>

      <ImportModal
        open={importOpen()}
        onClose={() => setImportOpen(false)}
        onImported={(summary) => void showNew(summary.importedIds)}
        onResult={(report) => {
          setImportOpen(false);
          next = () => setImportReport(report);
        }}
        onExitComplete={openNext}
      />
      <Sheet
        open={importReport() !== null}
        title={'Import finished'}
        onClose={() => setImportReport(null)}
      >
        <Show when={importReport()}>
          {(report) => (
            <div class={'px-5 pb-3 pt-2'}>
              <ImportResult report={report()} onDone={() => setImportReport(null)} inSheet />
            </div>
          )}
        </Show>
      </Sheet>
      <ExportVaultModal
        open={exportOpen()}
        onClose={() => setExportOpen(false)}
        onResult={() => {
          setExportOpen(false);
          next = () => setExportSaved(true);
        }}
        onExitComplete={openNext}
      />
      <Sheet open={exportSaved()} title={'Export finished'} onClose={() => setExportSaved(false)}>
        <div class={'px-5 pb-3 pt-2'}>
          <ExportSaved onDone={() => setExportSaved(false)} inSheet />
        </div>
      </Sheet>

      <AddEntryModal
        open={addOpen()}
        onClose={() => setAddOpen(false)}
        onSuccess={(id) => void showNew([id])}
      />

      <SettingsPage open={settingsOpen()} onClose={() => setSettingsOpen(false)} />

      <DeleteEntry
        entry={deleteTarget()}
        onClose={() => setDeleteTarget(null)}
        onDeleted={() => {
          setDeleteTarget(null);
          void list.refresh();
        }}
      />

      <ExportEntryQrModal
        entry={exportTarget()}
        onClose={() => setExportTarget(null)}
        onMissing={() => {
          setExportTarget(null);
          void list.refresh();
        }}
      />
    </div>
  );
};

function rowId(entryId: string): string {
  return `touch-entry-${entryId}`;
}

interface HeaderButtonProps {
  icon: string;
  label: string;
  onClick: () => void;
}

/** A 48 px icon button in the app bar: the desktop's quiet button, sized for a finger. */
const HeaderButton: Component<HeaderButtonProps> = (props) => (
  <button
    type={'button'}
    class={'btn-ghost text-text rounded-full shrink-0 size-12'}
    aria-label={props.label}
    onClick={() => props.onClick()}
  >
    <i class={`${props.icon} size-6`} aria-hidden={'true'} />
  </button>
);
