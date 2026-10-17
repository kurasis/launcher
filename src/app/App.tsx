import { useState } from 'react';
import { Button } from '../components/Button';
import { Notice } from '../components/Notice';
import { StatusBadge } from '../components/StatusBadge';
import { AccountChip } from '../features/accounts/AccountChip';
import { RunStatus } from '../features/diagnostics/RunStatus';
import { StorageIssues } from '../features/diagnostics/StorageIssues';
import { InstancesView } from '../features/instances/InstancesView';
import { SettingsView } from '../features/settings/SettingsView';
import { appStore, useAppStore } from '../lib/store';
import { strings } from '../lib/strings';
import { AboutView } from './AboutView';
import styles from './App.module.css';
import { QuitControl } from './QuitControl';
import { useTheme } from './useTheme';

type View = 'home' | 'settings' | 'about';
const views: View[] = ['home', 'settings', 'about'];

export function App() {
  const state = useAppStore();
  const [view, setView] = useState<View>('home');
  const snapshot = state.status === 'ready' ? state.snapshot : null;
  useTheme(snapshot?.settings.theme);

  return (
    <div className={styles.shell}>
      <header className={styles.topbar}>
        <div className={styles.brandRow}>
          <h1 className={styles.brand}>{strings.productName}</h1>
          <StatusBadge tone="neutral" label={strings.buildTag} />
        </div>
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
          <QuitControl />
        </nav>
      </header>

      <main className={styles.content}>
        {view === 'about' ? (
          <AboutView snapshot={snapshot} />
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
            {view === 'home' ? (
              <>
                <RunStatus current={state.snapshot.currentRun} last={state.snapshot.lastRun} />
                <InstancesView snapshot={state.snapshot} />
              </>
            ) : (
              <SettingsView snapshot={state.snapshot} />
            )}
          </>
        )}
      </main>

      <footer className={styles.footer}>
        <p>{strings.independentNotice}</p>
      </footer>
    </div>
  );
}
