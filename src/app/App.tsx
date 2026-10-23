import { useEffect, useState } from 'react';
import { Button } from '../components/Button';
import { Icon } from '../components/Icon';
import { Logo } from '../components/Logo';
import { Notice } from '../components/Notice';
import { StatusBadge } from '../components/StatusBadge';
import { AccountView } from '../features/accounts/AccountView';
import { RunStatus } from '../features/diagnostics/RunStatus';
import { progressPercent } from '../lib/format';
import { useShownError } from '../lib/useShownError';
import { ipc } from '../lib/ipc';
import { StorageIssues } from '../features/diagnostics/StorageIssues';
import { InstancesView, type CreateRequest } from '../features/instances/InstancesView';
import { SettingsView } from '../features/settings/SettingsView';
import type { AppSnapshot, NetworkStatus } from '../lib/contracts';
import { DiagnosticsView } from '../features/diagnostics/DiagnosticsView';
import { appStore, useAppStore } from '../lib/store';
import { type SettingsSection, strings } from '../lib/strings';
import { useLanguage } from '../lib/useLanguage';
import { mainActionOf } from '../features/instances/mainAction';
import { AboutView } from './AboutView';
import styles from './App.module.css';
import { CommandPalette } from './CommandPalette';
import { LaunchBar, type LaunchRequest } from './LaunchBar';
import type { PaletteTarget } from './palette';
import { QuitControl } from './QuitControl';
import { RailUser } from './RailUser';
import { BackupsView } from '../features/backups/BackupsView';
import { GalleryView } from '../features/gallery/GalleryView';
import { ReviewsView } from '../features/reviews/ReviewsView';
import { onOpenReview, type ReviewPrefill } from '../features/reviews/openReview';
import { FailureOffer } from '../features/reports/FailureOffer';
import { UpdateBadge } from '../features/updates/UpdateBadge';
import { WhatsNew } from '../features/updates/WhatsNew';
import { DownloadManager } from '../features/downloads/DownloadManager';
import { FriendsCorner } from '../features/friends/FriendsCorner';
import { LaunchCheck } from '../features/mods/LaunchCheck';
import { MeteredApproval } from '../features/network/MeteredApproval';
import { ModsView, type ModsSection } from '../features/mods/ModsView';
import { WorldsView } from '../features/worlds/WorldsView';
import { FeedView } from '../features/feed/FeedView';
import { SkinsView } from '../features/skins/SkinsView';
import { StatsNotice } from '../features/settings/StatsNotice';
import { useTheme } from './useTheme';
import { useAppInfo } from './useAppInfo';
import { MoreMenu, type MenuItem } from './MoreMenu';
import {
  type View,
  VIEW_ICONS,
  menuEntries,
  menuViews,
  rememberedView,
  rowViews,
  startViewOf,
  viewShown,
} from './navigation';
import { localizeError } from '../lib/strings';

/**
 * Compact status of the running operation in the rail, visible on every screen. An
 * install or repair shows in the Downloads row and the launch bar instead.
 */
function ActivityIndicator({ snapshot }: { snapshot: AppSnapshot }) {
  const active = snapshot.activeOperation;
  if (active === null || active.kind === 'install' || active.kind === 'repair') return null;
  const progress = snapshot.operationProgress;
  const percent = progressPercent(progress?.operationId === active.operationId ? progress : null);
  return (
    <p className={styles.activity} role="status">
      <StatusBadge
        tone="neutral"
        label={strings.activity[active.kind]}
        className={styles.railChip}
      />
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
      <StatusBadge tone="warning" label={strings.network.offline} className={styles.railChip} />
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
  const [modsSection, setModsSection] = useState<ModsSection>('mod');
  const [modsInstance, setModsInstance] = useState<string | null>(null);
  // The new-installation form asked for from elsewhere (the feed's Install, the
  // command palette): Installations opens it, keyed by when it was asked.
  const [createRequest, setCreateRequest] = useState<(CreateRequest & { at: number }) | null>(null);
  // A room joined from a chat invite: Worlds opens on it.
  const [worldsRoom, setWorldsRoom] = useState<{ id: string; at: number } | null>(null);
  // A mod's page asked for from the command palette: Mods opens on it.
  const [modsProject, setModsProject] = useState<{ id: string; at: number } | null>(null);
  // Counts navigations: Installations and Mods start afresh on each one, so a form or
  // a mod page left open gives way to what was asked for (from the rail or the palette).
  const [visits, setVisits] = useState(0);
  // The command palette (Ctrl+K, ⌘K on macOS), and a main action it asked the launch bar to run.
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [launchRequest, setLaunchRequest] = useState<LaunchRequest | null>(null);
  // A review started from elsewhere (a crash, a failure, an ad), and the
  // answers to reviews not read yet (D-070).
  const [review, setReview] = useState<{ prefill: ReviewPrefill; at: number } | null>(null);
  const [unseenAnswers, setUnseenAnswers] = useState(0);
  const snapshot = state.status === 'ready' ? state.snapshot : null;
  // The start screen is picked once, from the first state the core sends.
  if (chosenView === null && snapshot !== null) setView(startViewOf(snapshot));
  const settings = snapshot?.settings;
  // The feed switched off while open gives way to Installations.
  const view = chosenView !== null && viewShown(chosenView, settings) ? chosenView : 'home';
  const tabs = rowViews.filter((item) => viewShown(item, settings));
  /** Opens a screen; with "Last opened" the core remembers it for the next start. */
  const navigate = (next: View) => {
    setView(next);
    setVisits((count) => count + 1);
    setCreateRequest(null);
    setModsProject(null);
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
  /** Settings → Privacy, from the line about the statistics (D-071). */
  const openPrivacy = () => {
    setSettingsSection('privacy');
    navigate('settings');
  };
  const openConsole = () => {
    clearConsoleError();
    void ipc.openConsole().catch(consoleFailed);
  };
  // macOS takes ⌘ where the others take Ctrl; there Ctrl+K deletes to the end of a line.
  const macos = appInfo.status === 'ready' && appInfo.info.os === 'macos';
  /**
   * Makes an installation the chosen one, as Installations does; false when the
   * settings cannot be written or the change failed.
   */
  const chooseInstallation = async (instanceId: string): Promise<boolean> => {
    if (snapshot === null) return false;
    if (instanceId === snapshot.settings.selectedInstanceId) return true;
    if (!snapshot.settings.writable) return false;
    try {
      await ipc.setAppSettings({ selectedInstanceId: instanceId });
    } catch {
      return false;
    }
    // Events normally bring the change; a refresh covers a missed one.
    appStore.refresh();
    return true;
  };
  /** Carries out what was chosen in the command palette, which closes first. */
  const runPalette = (target: PaletteTarget, main: boolean) => {
    setPaletteOpen(false);
    switch (target.kind) {
      case 'view':
        if (target.view === 'reviews') setReview(null);
        navigate(target.view);
        return;
      case 'console':
        openConsole();
        return;
      case 'settings':
        setSettingsSection(target.section);
        navigate('settings');
        return;
      case 'modsSection':
        setModsSection(target.section);
        navigate('mods');
        return;
      case 'newInstallation':
        // After navigate, which clears the request.
        navigate('home');
        setCreateRequest({ version: null, loader: target.loader, at: Date.now() });
        return;
      case 'installation': {
        const instance = snapshot?.instances.find((i) => i.id === target.instanceId);
        if (snapshot === null || instance === undefined) return;
        const chosen = instance.id === snapshot.settings.selectedInstanceId;
        if (!main || (!chosen && !snapshot.settings.writable)) {
          navigate('home');
          void chooseInstallation(instance.id);
          return;
        }
        // The main action runs in the launch bar, where its progress and errors show,
        // once the installation is the chosen one. Launching can ask about the mods
        // in a dialog: the palette is closed by then.
        const runs = mainActionOf(instance, snapshot).enabled;
        void chooseInstallation(instance.id).then((ok) => {
          if (ok && runs) setLaunchRequest({ instanceId: instance.id, at: Date.now() });
        });
        return;
      }
      case 'installedMod':
        navigate('mods');
        setModsInstance(target.instanceId);
        setModsSection('installed');
        if (target.projectId !== null) setModsProject({ id: target.projectId, at: Date.now() });
        return;
      case 'catalogMod':
        navigate('mods');
        if (modsInstance === null) setModsInstance(snapshot?.settings.selectedInstanceId ?? null);
        setModsSection('mod');
        setModsProject({ id: target.projectId, at: Date.now() });
        return;
    }
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
              if (entry === 'reviews') setReview(null);
              navigate(entry);
            },
          },
  );
  const inMenu = menuViews.includes(view);
  // Answers to reviews: asked at start (at most every 6 hours, by the core)
  // and when a screen asks to write one.
  const ready = state.status === 'ready';
  useEffect(() => {
    if (!ready) return;
    void ipc
      .reviewsMine()
      .then((mine) => {
        setUnseenAnswers(mine.unseen);
      })
      .catch(() => undefined);
  }, [ready]);
  // The review screen is a menu screen: never remembered as the start one.
  useEffect(
    () =>
      onOpenReview((prefill) => {
        setReview({ prefill, at: Date.now() });
        setView('reviews');
      }),
    [],
  );
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
  // Ctrl+K (⌘K on macOS) opens or closes the command palette, never over another
  // modal dialog. A key of this window only: no shortcut reaches the system or the game.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.code !== 'KeyK' || event.shiftKey || event.altKey || event.isComposing) return;
      if (macos ? !event.metaKey || event.ctrlKey : !event.ctrlKey || event.metaKey) return;
      event.preventDefault();
      if (event.repeat) return;
      if (paletteOpen) setPaletteOpen(false);
      else if (document.querySelector('[aria-modal="true"]') === null) setPaletteOpen(true);
    };
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('keydown', onKey);
    };
  }, [macos, paletteOpen]);

  return (
    <div className={styles.shell} key={lang}>
      {/* Concept A: the rail on the inline-start side holds the brand, the screens,
          friends, downloads, More and who plays; the launch bar sits at the foot. */}
      <header className={styles.rail}>
        <div className={styles.brandRow}>
          <Logo />
          <h1 className={styles.brand}>
            <span className={styles.brandName}>{strings.brand.name}</span>{' '}
            <span className={styles.brandSuffix}>{strings.brand.suffix}</span>
          </h1>
        </div>
        {/* Find: the command palette, outside the main nav (it is not a screen). */}
        <button
          type="button"
          className={[styles.navItem, styles.find].join(' ')}
          aria-haspopup="dialog"
          aria-expanded={paletteOpen}
          aria-keyshortcuts={macos ? 'Meta+K' : 'Control+K'}
          title={strings.palette.findHint({ keys: macos ? '⌘K' : 'Ctrl+K' })}
          onClick={(event) => {
            // Focus comes back here when the palette closes, on macOS too.
            event.currentTarget.focus();
            setPaletteOpen(true);
          }}
        >
          <Icon name="search" />
          <span className={styles.navLabel}>{strings.palette.find}</span>
          {/* Pushed to the end in the rail's direction; the keys themselves read left to right. */}
          <span className={styles.findKeys} aria-hidden="true">
            <span className={styles.findKeyRow} dir="ltr">
              <kbd>{macos ? '⌘' : 'Ctrl'}</kbd>
              <kbd>K</kbd>
            </span>
          </span>
        </button>
        {appInfo.status === 'ready' && appInfo.info.buildProfile === 'debug' && (
          <StatusBadge tone="neutral" label={strings.devBuildTag} className={styles.railChip} />
        )}
        {appInfo.status === 'ready' && appInfo.info.testProfile !== null && (
          <StatusBadge
            tone="warning"
            label={strings.testProfileTag({ name: appInfo.info.testProfile })}
            className={styles.railChip}
          />
        )}
        <nav aria-label={strings.nav.label} className={styles.nav}>
          {tabs.map((item) => {
            const icon = VIEW_ICONS[item];
            return (
              <button
                key={item}
                type="button"
                className={styles.navItem}
                aria-current={view === item ? 'page' : undefined}
                title={strings.nav[item]}
                onClick={() => {
                  navigate(item);
                }}
              >
                <Icon name={icon} />
                <span className={styles.navLabel}>{strings.nav[item]}</span>
              </button>
            );
          })}
        </nav>
        <div className={styles.railEnd}>
          {snapshot !== null && <ActivityIndicator snapshot={snapshot} />}
          {snapshot !== null && <NetworkIndicator network={snapshot.network} />}
          <nav aria-label={strings.nav.tools} className={styles.tools}>
            {snapshot !== null && (
              <FriendsCorner
                snapshot={snapshot}
                onOpenWorld={(id) => {
                  setWorldsRoom({ id, at: Date.now() });
                  navigate('worlds');
                }}
                onOpenAccounts={() => {
                  navigate('account');
                }}
              />
            )}
            {snapshot !== null && <DownloadManager snapshot={snapshot} />}
            <MoreMenu
              label={
                inMenu ? strings.nav.moreCurrent({ view: strings.nav[view] }) : strings.nav.more
              }
              current={inMenu}
              entries={menuItems}
              news={unseenAnswers > 0 ? strings.nav.newAnswers({ count: unseenAnswers }) : null}
              icon={<Icon name="more" />}
              menuClassName={styles.railMenu}
            />
          </nav>
          {snapshot !== null && <UpdateBadge update={snapshot.update} />}
          {snapshot !== null ? (
            <RailUser
              account={snapshot.account}
              activeKind={snapshot.activeOperation?.kind ?? null}
            />
          ) : (
            // Quit and its app-menu listener stay while the core is loading or failed.
            <div className={styles.user}>
              <QuitControl activeKind={null} compact />
            </div>
          )}
        </div>
      </header>

      <div className={styles.column}>
        {/* Focusable from code: focus lands here when what the palette was opened from is gone. */}
        <main className={styles.content} tabIndex={-1}>
          {consoleOpening.error !== null && <Notice tone="danger">{consoleOpening.error}</Notice>}
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
              <p>
                {state.snapshot.dataRoot.error
                  ? localizeError(state.snapshot.dataRoot.error)
                  : null}
              </p>
            </Notice>
          ) : (
            <>
              <StorageIssues issues={state.snapshot.storageIssues} />
              {view === 'feed' && (
                <FeedView
                  snapshot={state.snapshot}
                  onOpenInstallations={(instanceId) => {
                    navigate('home');
                    if (
                      instanceId !== null &&
                      instanceId !== state.snapshot.settings.selectedInstanceId
                    ) {
                      void ipc
                        .setAppSettings({ selectedInstanceId: instanceId })
                        .catch(() => undefined);
                    }
                  }}
                  onInstallVersion={(versionId) => {
                    navigate('home');
                    setCreateRequest({ version: versionId, loader: null, at: Date.now() });
                  }}
                />
              )}
              {view === 'feed' && (
                <StatsNotice settings={state.snapshot.settings} onSettings={openPrivacy} />
              )}
              {view === 'home' && <WhatsNew update={state.snapshot.update} />}
              {view === 'home' && <FailureOffer />}
              {view === 'home' && (
                <>
                  <RunStatus
                    current={state.snapshot.currentRun}
                    last={state.snapshot.lastRun}
                    names={new Map(state.snapshot.instances.map((i) => [i.id, i.displayName]))}
                    onStop={(runId) => {
                      stopping.clear();
                      void ipc.stopGame(runId).catch(stopping.catcher);
                    }}
                  />
                  {stopping.error !== null && <Notice tone="danger">{stopping.error}</Notice>}
                  <InstancesView
                    key={`${String(visits)}:${String(createRequest?.at ?? 0)}`}
                    snapshot={state.snapshot}
                    createRequest={createRequest}
                    onEditSettings={(instanceId) => {
                      setSettingsInstance(instanceId);
                      setSettingsSection('installations');
                      navigate('settings');
                    }}
                    onOpenBackups={() => {
                      navigate('backups');
                    }}
                    onOpenContent={(instanceId) => {
                      setModsInstance(instanceId);
                      setModsSection('installed');
                      navigate('mods');
                    }}
                  />
                  {!state.snapshot.settings.feed.enabled && (
                    <StatsNotice settings={state.snapshot.settings} onSettings={openPrivacy} />
                  )}
                </>
              )}
              {view === 'mods' && (
                <ModsView
                  key={`${String(visits)}:${String(modsProject?.at ?? 0)}`}
                  initialProject={modsProject?.id ?? null}
                  snapshot={state.snapshot}
                  instanceId={modsInstance}
                  section={modsSection}
                  onSection={setModsSection}
                  onInstance={setModsInstance}
                />
              )}
              {view === 'worlds' && (
                <WorldsView
                  key={worldsRoom?.at ?? 0}
                  initialRoom={worldsRoom?.id ?? null}
                  snapshot={state.snapshot}
                  onOpenAccounts={() => {
                    navigate('account');
                  }}
                />
              )}
              {view === 'skins' && (
                <SkinsView
                  snapshot={state.snapshot}
                  onOpenAccounts={() => {
                    navigate('account');
                  }}
                />
              )}
              {view === 'account' && (
                <AccountView
                  snapshot={state.snapshot}
                  onOpenSkins={() => {
                    navigate('skins');
                  }}
                />
              )}
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
              {view === 'reviews' && (
                <ReviewsView
                  key={review?.at ?? 0}
                  snapshot={state.snapshot}
                  prefill={review?.prefill ?? null}
                  onUnseen={setUnseenAnswers}
                />
              )}
            </>
          )}
        </main>

        {snapshot !== null && <MeteredApproval snapshot={snapshot} />}
        <LaunchCheck
          onOpenMods={(instanceId) => {
            setModsInstance(instanceId);
            setModsSection('installed');
            navigate('mods');
          }}
        />
        <footer className={styles.footer}>
          <p>{strings.independentNotice}</p>
        </footer>
        {snapshot !== null && (
          <LaunchBar
            snapshot={snapshot}
            request={launchRequest ?? undefined}
            onOpenInstallations={
              view === 'home'
                ? undefined
                : () => {
                    navigate('home');
                  }
            }
          />
        )}
      </div>
      {paletteOpen && (
        <CommandPalette
          snapshot={snapshot}
          macos={macos}
          onClose={() => {
            setPaletteOpen(false);
          }}
          onChoose={runPalette}
        />
      )}
    </div>
  );
}
