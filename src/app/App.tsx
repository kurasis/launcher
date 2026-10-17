import { useEffect, useState } from 'react';
import { Button } from '../components/Button';
import { Logo } from '../components/Logo';
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
import { type SettingsSection, strings } from '../lib/strings';
import { useLanguage } from '../lib/useLanguage';
import { AboutView } from './AboutView';
import styles from './App.module.css';
import { QuitControl } from './QuitControl';
import { useTheme } from './useTheme';
import { useAppInfo } from './useAppInfo';
import { localizeError } from '../lib/strings';

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
      {percent !== null && strings.activity.percent({ percent })}
    </p>
  );
}

export function App() {
  const state = useAppStore();
  // The Rust core's build profile: a debug core is a development build even
  // when its frontend was bundled.
  const { state: appInfo } = useAppInfo();
  const [view, setView] = useState<View>('home');
  const [settingsSection, setSettingsSection] = useState<SettingsSection>('general');
  const [settingsInstance, setSettingsInstance] = useState<string | null>(null);
  const snapshot = state.status === 'ready' ? state.snapshot : null;
  useTheme(snapshot?.settings.theme);
  // A language change (D-038) remounts the screens so every text is read again.
  const lang = useLanguage(snapshot?.settings.language);
  // Ctrl+Shift+L (Cmd+Shift+L on macOS) opens the console window (D-037).
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.shiftKey && event.code === 'KeyL') {
        event.preventDefault();
        void ipc.openConsole().catch(() => undefined);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('keydown', onKey);
    };
  }, []);

  return (
    <div className={styles.shell} key={lang}>
      <header className={styles.topbar}>
        <div className={styles.brandRow}>
          <Logo />
          <h1 className={styles.brand}>
            <span className={styles.brandName}>{strings.brand.name}</span>{' '}
            <span className={styles.brandSuffix}>{strings.brand.suffix}</span>
          </h1>
          {appInfo.status === 'ready' && appInfo.info.buildProfile === 'debug' && (
            <StatusBadge tone="neutral" label={strings.devBuildTag} />
          )}
          {appInfo.status === 'ready' && appInfo.info.testProfile !== null && (
            <StatusBadge
              tone="warning"
              label={strings.testProfileTag({ name: appInfo.info.testProfile })}
            />
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
            <p>
              {state.snapshot.dataRoot.error ? localizeError(state.snapshot.dataRoot.error) : null}
            </p>
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
                <InstancesView
                  snapshot={state.snapshot}
                  onEditSettings={(instanceId) => {
                    setSettingsInstance(instanceId);
                    setSettingsSection('installations');
                    setView('settings');
                  }}
                />
              </>
            )}
            {view === 'account' && <AccountView snapshot={state.snapshot} />}
            {view === 'settings' && (
              <SettingsView
                key={settingsInstance ?? ''}
                snapshot={state.snapshot}
                section={settingsSection}
                onSection={setSettingsSection}
                instanceId={settingsInstance}
              />
            )}
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
