import { type ReactNode, useCallback, useEffect, useId, useState } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import { StatusBadge } from '../../components/StatusBadge';
import type {
  AppSnapshot,
  Feed,
  FeedProblem,
  FeedSettings,
  FeedStream,
  FeedVideo,
  FeedView as FeedAnswer,
  InstanceSummary,
  NoteCard,
} from '../../lib/contracts';
import { feedProblem, ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import { gameLabel } from '../instances/loaderLabel';
import { FeedList } from './FeedList';
import type { FeedFilter } from './list';
import { NotePanel } from './NotePanel';
import { kindLabel, visibleParts } from './parts';
import { pictureUrl } from './pictureUrl';
import { dateTime, relativeTime, shortDate } from './when';
import styles from './Feed.module.css';

const t = strings.feed;

const FILTERS: readonly FeedFilter[] = ['all', 'versions', 'news', 'videos', 'streams'];

interface FeedViewProps {
  snapshot: AppSnapshot;
  /** Opens Installations, with this installation selected when given. */
  onOpenInstallations: (instanceId: string | null) => void;
  /** Opens the new-installation form with this game version chosen. */
  onInstallVersion: (versionId: string) => void;
}

/** The installation played last, if any was. */
function lastPlayed(instances: InstanceSummary[]): InstanceSummary | null {
  let found: InstanceSummary | null = null;
  for (const instance of instances) {
    if (instance.lastPlayedAt === null) continue;
    if (found?.lastPlayedAt == null || instance.lastPlayedAt > found.lastPlayedAt) {
      found = instance;
    }
  }
  return found;
}

/** "Continue playing": the installation played last, with Play. */
function ContinueCard({
  snapshot,
  onOpenInstallations,
}: Pick<FeedViewProps, 'snapshot' | 'onOpenInstallations'>) {
  const play = useCommand((requestId: string, id: string) => ipc.launchGame(id, requestId));
  const instance = lastPlayed(snapshot.instances);
  const headingId = useId();
  if (instance === null || instance.lastPlayedAt === null) return null;
  const running = snapshot.currentRun !== null;
  const busy = snapshot.activeOperation !== null;
  const ready = instance.health === 'ready';
  return (
    <section className={styles.continue} aria-labelledby={headingId}>
      <div>
        <h2 id={headingId} className={styles.eyebrow}>
          {t.continueTitle}
        </h2>
        <p className={styles.continueName}>
          {instance.displayName} <span className={styles.muted}>{gameLabel(instance)}</span>
        </p>
        <p className={styles.muted}>
          {t.lastPlayed({ when: relativeTime(instance.lastPlayedAt) })}
        </p>
        {running && <p role="status">{t.gameRunning}</p>}
      </div>
      <div className={styles.actions}>
        {ready && !running && (
          <Button
            variant="primary"
            disabled={busy || play.pending}
            onClick={() => void play.run(instance.id)}
          >
            {play.pending ? t.starting : t.play}
          </Button>
        )}
        <Button
          variant={ready && !running ? 'secondary' : 'primary'}
          onClick={() => {
            onOpenInstallations(instance.id);
          }}
        >
          {t.openInstallations}
        </Button>
      </div>
      {play.error !== null && <Notice tone="danger">{play.error}</Notice>}
    </section>
  );
}

/** The newest version: its patch note, and a way to install it. */
function HeroCard({
  feed,
  note,
  onOpen,
  onInstallVersion,
}: {
  feed: Feed;
  note: NoteCard | null;
  onOpen: (card: NoteCard) => void;
  onInstallVersion: (versionId: string) => void;
}) {
  const headingId = useId();
  const release = feed.latest.release;
  const version = note !== null ? note.version : (release?.id ?? null);
  if (note === null && release === null) return null;
  const image = pictureUrl(note?.image ?? null);
  const kind = note !== null ? kindLabel(note) : t.kinds.release;
  const date = note?.date ?? release?.date ?? '';
  return (
    <section className={styles.hero} aria-labelledby={headingId}>
      {image !== null && <img className={styles.heroPicture} src={image} alt="" />}
      <div className={styles.heroText}>
        <p className={styles.eyebrow}>{t.newVersion}</p>
        <div className={styles.badges}>
          <StatusBadge tone="success" label={kind} />
          {version !== null && (
            <span className={styles.muted} dir="ltr">
              {version}
            </span>
          )}
          <span className={styles.muted}>{shortDate(date)}</span>
          {note?.translated === true && <StatusBadge tone="warning" label={t.machineTranslated} />}
        </div>
        <h2 id={headingId} className={styles.heroTitle}>
          {note?.title ?? t.versionTitle({ version: release?.id ?? '' })}
        </h2>
        {note !== null && note.summary !== '' && <p>{note.summary}</p>}
        <div className={styles.actions}>
          {note !== null && (
            <Button
              variant="primary"
              onClick={() => {
                onOpen(note);
              }}
            >
              {t.readNote}
            </Button>
          )}
          {version !== null && (
            <Button
              onClick={() => {
                onInstallVersion(version);
              }}
            >
              {t.install({ version })}
            </Button>
          )}
        </div>
      </div>
    </section>
  );
}

/** A titled part of the feed. */
function Part({
  title,
  children,
  empty,
}: {
  title: string;
  children: ReactNode;
  /** Shown instead when there is nothing. */
  empty: string | null;
}) {
  const headingId = useId();
  return (
    <section className={styles.part} aria-labelledby={headingId}>
      <h2 id={headingId} className={styles.partTitle}>
        {title}
      </h2>
      {empty !== null ? <p className={styles.muted}>{empty}</p> : children}
    </section>
  );
}

/** The update cycle, events, announcements and the tip. */
function Side({
  feed,
  showCycle,
  onLink,
}: {
  feed: Feed;
  showCycle: boolean;
  onLink: (url: string) => void;
}) {
  const cycle = showCycle ? feed.cycle : null;
  return (
    <aside className={styles.side} aria-label={t.sideLabel}>
      {cycle !== null && (
        <Part title={t.cycleTitle} empty={null}>
          <p className={styles.cycleTarget}>
            {cycle.released
              ? t.cycleReleased({ version: cycle.target })
              : t.cycleTarget({ version: cycle.target })}
          </p>
          <ul className={styles.counts}>
            <li>{t.cycleSnapshots({ count: cycle.snapshots })}</li>
            <li>{t.cyclePre({ count: cycle.pre })}</li>
            <li>{t.cycleRc({ count: cycle.rc })}</li>
          </ul>
          <p className={styles.muted}>
            {t.cycleLatest({ version: cycle.latest, when: relativeTime(cycle.date) })}
          </p>
        </Part>
      )}
      {feed.events.length > 0 && (
        <Part title={t.eventsTitle} empty={null}>
          <ul className={styles.sideList}>
            {feed.events.map((event) => (
              <li key={event.id}>
                <p className={styles.sideItemTitle}>{event.title}</p>
                <p className={styles.muted}>
                  {t.eventWhen({ starts: dateTime(event.starts), ends: dateTime(event.ends) })}
                </p>
                {event.url !== null && (
                  <button
                    type="button"
                    className={styles.link}
                    onClick={() => {
                      if (event.url !== null) onLink(event.url);
                    }}
                  >
                    {t.openLink}
                  </button>
                )}
              </li>
            ))}
          </ul>
        </Part>
      )}
      {feed.announcements.length > 0 && (
        <Part title={t.announcementsTitle} empty={null}>
          <ul className={styles.sideList}>
            {feed.announcements.map((item) => (
              <li key={item.id}>
                <p className={styles.sideItemTitle}>{item.title}</p>
                {item.text !== '' && <p>{item.text}</p>}
                {item.url !== null && (
                  <button
                    type="button"
                    className={styles.link}
                    onClick={() => {
                      if (item.url !== null) onLink(item.url);
                    }}
                  >
                    {t.openLink}
                  </button>
                )}
              </li>
            ))}
          </ul>
        </Part>
      )}
      {feed.tip !== null && (
        <Part title={t.tipTitle} empty={null}>
          <p>{feed.tip}</p>
        </Part>
      )}
    </aside>
  );
}

/**
 * The start page feed (D-067): the installation played last, the newest
 * version, patch notes and news, videos and streams, and our own events and
 * tips. Everything comes from the core, which asks only feed.lapi.sh.
 */
export function FeedView({ snapshot, onOpenInstallations, onInstallVersion }: FeedViewProps) {
  const settings = snapshot.settings.feed;
  const [result, setResult] = useState<{
    view: FeedAnswer | null;
    problem: FeedProblem | null;
  } | null>(null);
  const [own, setOwn] = useState<FeedVideo[]>([]);
  const [refreshing, setRefreshing] = useState(false);
  const [chosen, setFilter] = useState<FeedFilter>('all');
  // Each Refresh starts the list again.
  const [refreshes, setRefreshes] = useState(0);
  const [open, setOpen] = useState<NoteCard | null>(null);
  const [actionProblem, setActionProblem] = useState<FeedProblem | null>(null);
  const fetchFeed = useCallback(
    (refresh: boolean) =>
      ipc
        .feedGet(refresh)
        .then((view) => {
          setResult({ view, problem: null });
        })
        .catch((error: unknown) => {
          setResult((previous) => ({
            view: previous?.view ?? null,
            problem: feedProblem(error),
          }));
        }),
    [],
  );
  // Asked again when its languages change or the connection returns.
  const languages = `${snapshot.settings.language} ${String(settings.machineTranslation)} ${settings.videoLanguage}`;
  const online = snapshot.network !== 'offline';
  useEffect(() => {
    void fetchFeed(false);
  }, [fetchFeed, languages, online]);
  useEffect(() => {
    ipc
      .feedOwnVideos()
      .then(setOwn)
      .catch(() => {
        setOwn([]);
      });
  }, [online]);

  const openLink = useCallback((url: string) => {
    setActionProblem(null);
    ipc.feedOpenLink(url).catch((error: unknown) => {
      setActionProblem(feedProblem(error));
    });
  }, []);
  const playVideo = (video: FeedVideo) => {
    setActionProblem(null);
    ipc.feedPlay('yt', video.id, strings.windowTitles.player).catch((error: unknown) => {
      setActionProblem(feedProblem(error));
    });
  };
  const playStream = (stream: FeedStream) => {
    setActionProblem(null);
    ipc.feedPlay('twitch', stream.login, strings.windowTitles.player).catch((error: unknown) => {
      setActionProblem(feedProblem(error));
    });
  };
  const closeNote = useCallback(() => {
    setOpen(null);
  }, []);

  const view = result?.view ?? null;
  const toolbar = (
    <div className={styles.toolbar}>
      <h2 className={styles.screenTitle}>{t.title}</h2>
      <Button
        disabled={refreshing}
        onClick={() => {
          setRefreshing(true);
          void fetchFeed(true).finally(() => {
            setRefreshing(false);
            setRefreshes((count) => count + 1);
          });
        }}
      >
        {refreshing ? t.refreshing : t.refresh}
      </Button>
    </div>
  );

  const content =
    view === null ? null : (
      <FeedParts
        feed={view.feed}
        settings={settings}
        own={own}
        chosen={chosen}
        reload={`${languages} ${String(online)} ${String(settings.snapshots)} ${String(settings.shorts)} ${settings.hidden.join(',')} ${String(refreshes)}`}
        onFilter={setFilter}
        onOpen={setOpen}
        onInstallVersion={onInstallVersion}
        onPlayVideo={playVideo}
        onPlayStream={playStream}
      />
    );

  return (
    <div className={styles.screen}>
      {toolbar}
      {view?.stale === true && (
        <Notice tone="warning">{t.stale({ date: dateTime(view.feed.generated) })}</Notice>
      )}
      {view !== null && result?.problem != null && (
        <Notice tone="warning">{t.problems[result.problem]}</Notice>
      )}
      {actionProblem !== null && <Notice tone="danger">{t.problems[actionProblem]}</Notice>}
      <div className={styles.layout}>
        <div className={styles.main}>
          <ContinueCard snapshot={snapshot} onOpenInstallations={onOpenInstallations} />
          {content ??
            (result?.problem != null ? (
              <Notice tone="danger">
                <p>{t.problems[result.problem]}</p>
              </Notice>
            ) : (
              <p role="status">{t.loading}</p>
            ))}
        </div>
        {view !== null && (
          <Side feed={view.feed} showCycle={settings.snapshots} onLink={openLink} />
        )}
      </div>
      {open !== null && (
        <NotePanel key={open.id} card={open} onClose={closeNote} onLink={openLink} />
      )}
    </div>
  );
}

interface FeedPartsProps {
  feed: Feed;
  settings: FeedSettings;
  own: FeedVideo[];
  chosen: FeedFilter;
  /** Changes when the list should start again. */
  reload: string;
  onFilter: (filter: FeedFilter) => void;
  onOpen: (card: NoteCard) => void;
  onInstallVersion: (versionId: string) => void;
  onPlayVideo: (video: FeedVideo) => void;
  onPlayStream: (stream: FeedStream) => void;
}

/** The newest version, the chips and the endless list they choose. */
function FeedParts({
  feed,
  settings,
  own,
  chosen,
  reload,
  onFilter,
  onOpen,
  onInstallVersion,
  onPlayVideo,
  onPlayStream,
}: FeedPartsProps) {
  const filterLabel = useId();
  const parts = visibleParts(feed, settings);
  const filters = FILTERS.filter((item) => item === 'all' || !parts.hidden.has(item));
  const filter = filters.includes(chosen) ? chosen : 'all';
  const heroNote = parts.versions[0] ?? null;

  return (
    <>
      {filter === 'all' && (
        <HeroCard feed={feed} note={heroNote} onOpen={onOpen} onInstallVersion={onInstallVersion} />
      )}
      <div className={styles.chips} role="group" aria-labelledby={filterLabel}>
        <span id={filterLabel} className={styles.visuallyHidden}>
          {t.filters}
        </span>
        {filters.map((item) => (
          <button
            key={item}
            type="button"
            className={styles.chip}
            aria-pressed={filter === item}
            onClick={() => {
              onFilter(item);
            }}
          >
            {t.filter[item]}
          </button>
        ))}
      </div>
      <FeedList
        feed={feed}
        settings={settings}
        own={own}
        filter={filter}
        key={`${filter} ${reload}`}
        skip={filter === 'all' ? (heroNote?.id ?? null) : null}
        onOpen={onOpen}
        onPlayVideo={onPlayVideo}
        onPlayStream={onPlayStream}
      />
    </>
  );
}
