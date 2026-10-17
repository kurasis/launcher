import { useState } from 'react';
import { Button } from '../components/Button';
import { Notice } from '../components/Notice';
import { StatusBadge } from '../components/StatusBadge';
import { AccountChip } from '../features/accounts/AccountChip';
import { AccountView } from '../features/accounts/AccountView';
import { RunStatus } from '../features/diagnostics/RunStatus';
import { ipc } from '../lib/ipc';
import { StorageIssues } from '../features/diagnostics/StorageIssues';
import { InstancesView } from '../features/instances/InstancesView';
import { SettingsView } from '../features/settings/SettingsView';
import type { AppSnapshot } from '../lib/contracts';
import { DiagnosticsView } from '../features/diagnostics/DiagnosticsView';
import { appStore, useAppStore } from '../lib/store';
import { strings } from '../lib/strings';
import { AboutView } from './AboutView';
import styles from './App.module.css';
import { QuitControl } from './QuitControl';
import { useTheme } from './useTheme';
import { useAppInfo } from './useAppInfo';

type View = 'home' | 'account' | 'settings' | 'diagnostics' | 'about';
const views: View[] = ['home', 'account', 'settings', 'diagnostics', 'about'];

/** Compact header status of the running operation, visible on every screen. */
function ActivityIndicator({ snapshot }: { snapshot: AppSnapshot }) {
  const active = snapshot.activeOperation;
  if (active === null) return null;
  const progress = snapshot.operationProgress;
  const total = progress?.operationId === active.operationId ? progress.totalDownloadBytes : null;
  const percent =
    total !== null && total > 0 && progress !== null
      ? Math.min(100, Math.floor((progress.downloadedBytes / total) * 100))
      : null;
  return (
    <p className={styles.activity} role="status">
      <StatusBadge tone="neutral" label={strings.activity[active.kind]} />
      {percent !== null && strings.activity.percent(percent)}
    </p>
  );
}

export function App() {
  const state = useAppStore();
  // The Rust core's build profile: a debug core is a development build even
  // when its frontend was bundled.
  const { state: appInfo } = useAppInfo();
  const [view, setView] = useState<View>('home');
  const snapshot = state.status === 'ready' ? state.snapshot : null;
  useTheme(snapshot?.settings.theme);

  return (
    <div className={styles.shell}>
      <header className={styles.topbar}>
        <div className={styles.brandRow}>
          <h1 className={styles.brand}>{strings.productName}</h1>
          {appInfo.status === 'ready' && (
            <StatusBadge tone="neutral" label={strings.buildTag(appInfo.info.buildProfile)} />
          )}
          {appInfo.status === 'ready' && appInfo.info.testProfile !== null && (
            <StatusBadge tone="warning" label={strings.testProfileTag(appInfo.info.testProfile)} />
          )}
        </div>
        {snapshot !== null && <ActivityIndicator snapshot={snapshot} />}
        {snapshot !== null && <AccountChip account={snapshot.account} />}
        <nav aria-label={strings.nav.label} className={styles.nav}>
          {views.map((item) => (
            <button
              key={item}
              type="button"
              className={styles.navItem}
              aria-current={view === item ? 'page' : undefined}
              onClick={() => {
                setView(item);
              }}
            >
              {strings.nav[item]}
            </button>
          ))}
          <QuitControl activeKind={snapshot?.activeOperation?.kind ?? null} />
        </nav>
      </header>

      <main className={styles.content}>
        {view === 'about' ? (
          <AboutView />
        ) : state.status === 'loading' ? (
          <p role="status">{strings.bootstrap.loading}</p>
        ) : state.status === 'error' ? (
          <Notice tone="danger">
            <p>
              {strings.bootstrap.errorTitle}: {state.message}
            </p>
            <Button onClick={appStore.refresh}>{strings.bootstrap.retry}</Button>
          </Notice>
        ) : state.snapshot.dataRoot.status === 'unavailable' ? (
          <Notice tone="danger">
            <p>
              <strong>{strings.bootstrap.unavailableTitle}</strong>
            </p>
            <p>{state.snapshot.dataRoot.error?.message}</p>
          </Notice>
        ) : (
          <>
            <StorageIssues issues={state.snapshot.storageIssues} />
            {view === 'home' && (
              <>
                <RunStatus
                  current={state.snapshot.currentRun}
                  last={state.snapshot.lastRun}
                  onStop={(runId) => {
                    void ipc.stopGame(runId).catch(() => undefined);
                  }}
                />
                <InstancesView snapshot={state.snapshot} />
              </>
            )}
            {view === 'account' && <AccountView snapshot={state.snapshot} />}
            {view === 'settings' && <SettingsView snapshot={state.snapshot} />}
            {view === 'diagnostics' && <DiagnosticsView snapshot={state.snapshot} />}
          </>
        )}
      </main>

      <footer className={styles.footer}>
        <p>{strings.independentNotice}</p>
      </footer>
    </div>
  );
}
