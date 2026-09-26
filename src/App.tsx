import { createSignal, ErrorBoundary, Match, onCleanup, onMount, Switch } from 'solid-js';

import { reportUserActivity } from '@api/activity';
import { errorMessage } from '@api/lib';
import {
  detectVaultScreen,
  onBackendLock,
  startFailure,
  suppressPageContextMenu,
} from '@cpt/app-shell';
import { CreateVaultScreen, UnlockScreen } from '@cpt/auth-screens';
import { Dashboard } from '@cpt/dashboard';
import { ErrorPanel } from '@cpt/error-panel';
import { useSettings } from '@cpt/settings';
import { GlobalToaster } from '@cpt/toaster';

import type { Screen, StartFailure } from '@cpt/app-shell';
import type { Component, ParentComponent } from 'solid-js';

const App: Component = () => {
  // Created here, during the first render, so the cached theme is painted
  // before anything else is.
  useSettings();

  const [screen, setScreen] = createSignal<Screen>('starting');
  const [failure, setFailure] = createSignal<StartFailure | null>(null);

  async function start() {
    setFailure(null);
    setScreen('starting');
    try {
      setScreen(await detectVaultScreen());
    } catch (err) {
      setFailure(startFailure(err));
    }
  }

  onMount(() => {
    void start();
    // The backend owns auto-lock; the frontend only tells it the user is there.
    const stopReporting = reportUserActivity();
    onCleanup(stopReporting);
    onCleanup(suppressPageContextMenu());
  });

  // Only an unlocked vault can lock itself, so this always leaves the dashboard.
  onBackendLock(() => setScreen('unlock'));

  return (
    <>
      <main class={'text-text bg-bg-app h-full'}>
        <ErrorBoundary
          fallback={(err, reset) => (
            <Centered>
              <ErrorPanel
                title={'Something went wrong'}
                message={errorMessage(err)}
                onRetry={reset}
              />
            </Centered>
          )}
        >
          <Switch>
            <Match when={failure()}>
              {(current) => (
                <Centered>
                  <ErrorPanel
                    title={current().title}
                    message={current().message}
                    onRetry={() => void start()}
                  />
                </Centered>
              )}
            </Match>
            <Match when={screen() === 'starting'}>
              <Centered>
                <p class={'appear-delayed subtle'}>{'Opening the vault…'}</p>
              </Centered>
            </Match>
            <Match when={screen() === 'create'}>
              <CreateVaultScreen onDone={() => setScreen('unlocked')} />
            </Match>
            <Match when={screen() === 'unlock'}>
              <UnlockScreen onDone={() => setScreen('unlocked')} />
            </Match>
            <Match when={screen() === 'unlocked'}>
              <Dashboard onLocked={() => setScreen('unlock')} />
            </Match>
          </Switch>
        </ErrorBoundary>
      </main>
      <GlobalToaster />
    </>
  );
};

const Centered: ParentComponent = (props) => (
  <div class={'p-4 flex h-full items-center justify-center'}>{props.children}</div>
);

export default App;
