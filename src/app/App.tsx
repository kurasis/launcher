import { useEffect, useState } from 'react';
import { Button } from '../components/Button';
import { Logo } from '../components/Logo';
import { Notice } from '../components/Notice';
import { StatusBadge } from '../components/StatusBadge';
import { AccountChip } from '../features/accounts/AccountChip';
import { AccountView } from '../features/accounts/AccountView';
import { RunStatus } from '../features/diagnostics/RunStatus';
import { progressPercent } from '../lib/format';
import { useShownError } from '../lib/useShownError';
import { ipc } from '../lib/ipc';
import { StorageIssues } from '../features/diagnostics/StorageIssues';
import { InstancesView } from '../features/instances/InstancesView';
import { SettingsView } from '../features/settings/SettingsView';
import type { AppSnapshot, NetworkStatus } from '../lib/contracts';
import { DiagnosticsView } from '../features/diagnostics/DiagnosticsView';
import { appStore, useAppStore } from '../lib/store';
import { type SettingsSection, strings } from '../lib/strings';
import { useLanguage } from '../lib/useLanguage';
import { AboutView } from './AboutView';
import styles from './App.module.css';
import { QuitControl } from './QuitControl';
import { BackupsView } from '../features/backups/BackupsView';
import { GalleryView } from '../features/gallery/GalleryView';
import { FailureOffer } from '../features/reports/FailureOffer';
import { UpdateBadge } from '../features/updates/UpdateBadge';
import { WhatsNew } from '../features/updates/WhatsNew';
import { DownloadManager } from '../features/downloads/DownloadManager';
import { FriendsCorner } from '../features/friends/FriendsCorner';
import { useTheme } from './useTheme';
import { useAppInfo } from './useAppInfo';
import { MoreMenu, type MenuItem } from './MoreMenu';
import {
  type View,
  menuEntries,
  menuViews,
  rememberedView,
  rowViews,
  startViewOf,
} from './navigation';
import { localizeError } from '../lib/strings';

/** Compact header status of the running operation, visible on every screen. */
function ActivityIndicator({ snapshot }: { snapshot: AppSnapshot }) {
  const active = snapshot.activeOperation;
  if (active === null) return null;
  const progress = snapshot.operationProgress;
  const percent = progressPercent(progress?.operationId === active.operationId ? progress : null);
  return (
    <p className={styles.activity} role="status">
      <StatusBadge tone="neutral" label={strings.activity[active.kind]} />
      {percent !== null && strings.activity.percent({ percent })}
    </p>
  );
}

/** Shown while the internet cannot be reached (D-052), with a way to check again. */
function NetworkIndicator({ network }: { network: NetworkStatus }) {
  const checking = useShownError();
  if (network !== 'offline') return null;
  return (
    <p
      className={styles.activity}
      role="status"
      title={checking.error ?? strings.network.offlineHint}
    >
      <StatusBadge tone="warning" label={strings.network.offline} />
      <button
        type="button"
        className={styles.textButton}
        onClick={() => {
          checking.clear();
          void ipc.checkNetwork().catch(checking.catcher);
        }}
      >
        {strings.network.check}
      </button>
    </p>
  );
}

export function App() {
  const state = useAppStore();
  // The Rust core's build profile: a debug core is a development build even
  // when its frontend was bundled.
  const { state: appInfo } = useAppInfo();
  // The screen the player chose; null until the start screen is known.
  const [chosenView, setView] = useState<View | null>(null);
  const stopping = useShownError();
  const consoleOpening = useShownError();
  const { clear: clearConsoleError, catcher: consoleFailed } = consoleOpening;
  const [settingsSection, setSettingsSection] = useState<SettingsSection>('general');
  const [settingsInstance, setSettingsInstance] = useState<string | null>(null);
  const snapshot = state.status === 'ready' ? state.snapshot : null;
  // The start screen is picked once, from the first state the core sends.
  if (chosenView === null && snapshot !== null) setView(startViewOf(snapshot));
  const view = chosenView ?? 'home';
  const settings = snapshot?.settings;
  /** Opens a screen; with "Last opened" the core remembers it for the next start. */
  const navigate = (next: View) => {
    setView(next);
    const remembered = rememberedView(next);
    if (
      settings?.startView === 'last' &&
      settings.writable &&
      remembered !== null &&
      remembered !== settings.lastView
    ) {
      // Only a convenience: a failure leaves the previous screen remembered.
      void ipc.setAppSettings({ lastView: remembered }).catch(() => undefined);
    }
  };
  const openConsole = () => {
    clearConsoleError();
    void ipc.openConsole().catch(consoleFailed);
  };
  const menuItems: MenuItem[] = menuEntries.map((entry, index) =>
    entry === 'separator'
      ? { kind: 'separator', key: `separator-${String(index)}` }
      : entry === 'console'
        ? {
            kind: 'item',
            key: entry,
            label: strings.nav.console,
            current: false,
            onSelect: openConsole,
          }
        : {
            kind: 'item',
            key: entry,
            label: strings.nav[entry],
            current: view === entry,
            onSelect: () => {
              navigate(entry);
            },
          },
  );
  const inMenu = menuViews.includes(view);
  useTheme(snapshot?.settings.theme);
  // A language change (D-038) remounts the screens so every text is read again.
  const lang = useLanguage(snapshot?.settings.language);
  // Once the interface shows real content, a new version's start counts as
  // good; one that never gets here is left after a few starts (D-063).
  const shown = snapshot !== null;
  useEffect(() => {
    if (shown) void ipc.interfaceReady().catch(() => undefined);
  }, [shown]);
  // Ctrl+Shift+L (Cmd+Shift+L on macOS) opens the console window (D-037).
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.shiftKey && event.code === 'KeyL') {
        event.preventDefault();
        clearConsoleError();
        void ipc.openConsole().catch(consoleFailed);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('keydown', onKey);
    };
  }, [clearConsoleError, consoleFailed]);

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
        {snapshot !== null && <NetworkIndicator network={snapshot.network} />}
        {snapshot !== null && <UpdateBadge update={snapshot.update} />}
        {snapshot !== null && <AccountChip account={snapshot.account} />}
        <nav aria-label={strings.nav.label} className={styles.nav}>
          {rowViews.map((item) => (
            <button
              key={item}
              type="button"
              className={styles.navItem}
              aria-current={view === item ? 'page' : undefined}
              onClick={() => {
                navigate(item);
              }}
            >
              {strings.nav[item]}
            </button>
          ))}
          <MoreMenu
            label={inMenu ? strings.nav.moreCurrent({ view: strings.nav[view] }) : strings.nav.more}
            current={inMenu}
            entries={menuItems}
          />
          <QuitControl activeKind={snapshot?.activeOperation?.kind ?? null} />
        </nav>
      </header>

      <main className={styles.content}>
        {consoleOpening.error !== null && <Notice tone="danger">{consoleOpening.error}</Notice>}
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
            {view === 'home' && <WhatsNew update={state.snapshot.update} />}
            {view === 'home' && <FailureOffer />}
            {view === 'home' && (
              <>
                <RunStatus
                  current={state.snapshot.currentRun}
                  last={state.snapshot.lastRun}
                  onStop={(runId) => {
                    stopping.clear();
                    void ipc.stopGame(runId).catch(stopping.catcher);
                  }}
                />
                {stopping.error !== null && <Notice tone="danger">{stopping.error}</Notice>}
                <InstancesView
                  snapshot={state.snapshot}
                  onEditSettings={(instanceId) => {
                    setSettingsInstance(instanceId);
                    setSettingsSection('installations');
                    navigate('settings');
                  }}
                  onOpenBackups={() => {
                    navigate('backups');
                  }}
                />
              </>
            )}
            {view === 'account' && <AccountView snapshot={state.snapshot} />}
            {view === 'backups' && <BackupsView snapshot={state.snapshot} />}
            {view === 'screenshots' && <GalleryView snapshot={state.snapshot} />}
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

      {snapshot !== null && <DownloadManager snapshot={snapshot} />}
      {snapshot !== null && (
        <FriendsCorner
          onOpenAccounts={() => {
            navigate('account');
          }}
        />
      )}
      <footer className={styles.footer}>
        <p>{strings.independentNotice}</p>
      </footer>
    </div>
  );
}
