import {
  type ReactNode,
  type Ref,
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
} from 'react';
import { Button } from '../../components/Button';
import { Icon } from '../../components/Icon';
import { Notice } from '../../components/Notice';
import type {
  AppSnapshot,
  ChannelMark,
  FavoriteFeed,
  Feed,
  FeedProblem,
  FeedSettings,
  FeedStream,
  FeedVideo,
  FeedMarks,
  FeedView as FeedAnswer,
  MarkKind,
  MarkList,
  NoteCard,
  ServerAd,
} from '../../lib/contracts';
import { feedProblem, ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { AddChannel } from './AddChannel';
import { recallFeed, rememberFeed } from './feedMemory';
import { FeedList, FeedLoading, type ListHandle } from './FeedList';
import type { FeedFilter } from './list';
import { NotePanel } from './NotePanel';
import { visibleParts } from './parts';
import { dateTime, relativeTime } from './when';
import styles from './Feed.module.css';

const t = strings.feed;

const FILTERS: readonly FeedFilter[] = ['all', 'versions', 'news', 'videos', 'shorts', 'streams'];
const NO_MARKS: FeedMarks = { hidden: [], favorite: [] };
const NO_FAVORITES: FavoriteFeed = { videos: [], streams: [] };
/** How long a short notice (link copied) stays. */
const NOTICE_MS = 4000;
/** How often live streams are asked again while the feed is shown. */
const LIVE_EVERY_MS = 3 * 60 * 1000;

/** A notice over the feed, with Undo when the step can be taken back. */
interface FeedNotice {
  text: string;
  undo: (() => void) | null;
}

/** What Refresh found, shown for a moment over the launch bar. */
interface Toast {
  text: string;
  failed: boolean;
}

interface FeedViewProps {
  snapshot: AppSnapshot;
  /** Opens Installations, with this installation selected when given. */
  onOpenInstallations: (instanceId: string | null) => void;
  /** Opens the new-installation form with this game version chosen. */
  onInstallVersion: (versionId: string) => void;
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
  if (
    cycle === null &&
    feed.events.length === 0 &&
    feed.announcements.length === 0 &&
    feed.tip === null
  ) {
    return null;
  }
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
 * The start page feed (D-067), as a mosaic (D-072): the newest version, patch
 * notes and news, videos and streams, and our own events and tips. Play for the
 * chosen installation is in the launch bar. Everything comes from the core,
 * which asks only feed.lapi.sh.
 */
export function FeedView({ snapshot, onOpenInstallations, onInstallVersion }: FeedViewProps) {
  const settings = snapshot.settings.feed;
  // The feed as it was left earlier in this run: shown at once, then asked again
  // quietly, so going back to it does not build it anew.
  const [place] = useState(recallFeed);
  const [result, setResult] = useState<{
    view: FeedAnswer | null;
    problem: FeedProblem | null;
  } | null>(place?.result ?? null);
  const [own, setOwn] = useState<FeedVideo[]>(place?.own ?? []);
  const [ownStreams, setOwnStreams] = useState<FeedStream[]>(place?.ownStreams ?? []);
  const [ads, setAds] = useState<ServerAd[]>(place?.ads ?? []);
  const [marks, setMarks] = useState<FeedMarks>(place?.marks ?? NO_MARKS);
  const [favorites, setFavorites] = useState<FavoriteFeed>(place?.favorites ?? NO_FAVORITES);
  const [notice, setNotice] = useState<FeedNotice | null>(null);
  const [toast, setToast] = useState<Toast | null>(null);
  const list = useRef<ListHandle>(null);
  // Each channel added from the + asks for the player's own videos and streams again.
  const [added, setAdded] = useState(0);
  // Every few minutes while the launcher is shown: live streams are asked again.
  const [liveTick, setLiveTick] = useState(0);
  const [watched, setWatched] = useState<ReadonlySet<string>>(place?.watched ?? new Set());
  const [refreshing, setRefreshing] = useState(false);
  const [chosen, setFilter] = useState<FeedFilter>(place?.filter ?? 'all');
  // Each Refresh asks for streams, favourites and ads again.
  const [refreshes, setRefreshes] = useState(0);
  const [open, setOpen] = useState<NoteCard | null>(null);
  const [actionProblem, setActionProblem] = useState<FeedProblem | null>(null);
  const fetchFeed = useCallback(
    (refresh: boolean) =>
      ipc
        .feedGet(refresh)
        .then((view) => {
          setResult({ view, problem: null });
          return true;
        })
        .catch((error: unknown) => {
          setResult((previous) => ({
            view: previous?.view ?? null,
            problem: feedProblem(error),
          }));
          return false;
        }),
    [],
  );

  // Kept for the rest of the run when the player goes to another screen, and
  // scrolled back to where they were when they return.
  const latest = useRef({ result, own, ownStreams, ads, marks, favorites, watched, chosen });
  useLayoutEffect(() => {
    latest.current = { result, own, ownStreams, ads, marks, favorites, watched, chosen };
  });
  useLayoutEffect(
    () => () => {
      const now = latest.current;
      if (now.result?.view == null) return;
      rememberFeed({
        ...now,
        result: { view: now.result.view, problem: null },
        filter: now.chosen,
        scrollY: window.scrollY,
      });
    },
    [],
  );
  useLayoutEffect(() => {
    if (place !== null && place.scrollY > 0) window.scrollTo(0, place.scrollY);
  }, [place]);
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
  }, [online, added]);
  // Live now: asked again on Refresh too.
  useEffect(() => {
    ipc
      .feedOwnStreams()
      .then(setOwnStreams)
      .catch(() => {
        setOwnStreams([]);
      });
  }, [online, refreshes, added, liveTick]);
  useEffect(() => {
    ipc
      .feedMarks()
      .then(setMarks)
      .catch(() => {
        setMarks(NO_MARKS);
      });
  }, []);
  // The favourites' videos and streams: asked again when the favourites change.
  const favoriteKey = marks.favorite.map((mark) => `${mark.kind}:${mark.id}`).join(',');
  useEffect(() => {
    ipc
      .feedFavorites()
      .then(setFavorites)
      .catch(() => {
        setFavorites(NO_FAVORITES);
      });
  }, [favoriteKey, online, refreshes, liveTick]);
  useEffect(() => {
    if (!online) return;
    const timer = window.setInterval(() => {
      if (document.visibilityState === 'visible') setLiveTick((tick) => tick + 1);
    }, LIVE_EVERY_MS);
    return () => {
      window.clearInterval(timer);
    };
  }, [online]);
  useEffect(() => {
    ipc
      .feedWatched()
      .then((ids) => {
        setWatched(new Set(ids));
      })
      .catch(() => undefined);
  }, []);
  // A notice without Undo goes by itself, and so does what Refresh found.
  useEffect(() => {
    if (notice === null || notice.undo !== null) return;
    const timer = window.setTimeout(() => {
      setNotice(null);
    }, NOTICE_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [notice]);
  useEffect(() => {
    if (toast === null) return;
    const timer = window.setTimeout(() => {
      setToast(null);
    }, NOTICE_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [toast]);

  // Server ads (D-069): asked again on Refresh and when the connection
  // returns; each leaves the list the moment it ends.
  useEffect(() => {
    ipc
      .feedServerAds('feed', refreshes > 0)
      .then((view) => {
        setAds(view.ads);
      })
      .catch(() => {
        setAds((shown) => (shown.length === 0 ? shown : []));
      });
  }, [online, refreshes, languages]);
  useEffect(() => {
    if (ads.length === 0) return;
    const soonest = Math.min(...ads.map((ad) => ad.endsAt)) * 1000 - Date.now();
    const timer = window.setTimeout(
      () => {
        setAds((shown) => shown.filter((ad) => ad.endsAt * 1000 > Date.now()));
      },
      Math.max(0, Math.min(soonest, 2 ** 31 - 1)),
    );
    return () => {
      window.clearTimeout(timer);
    };
  }, [ads]);

  const openLink = useCallback((url: string) => {
    setActionProblem(null);
    ipc.feedOpenLink(url).catch((error: unknown) => {
      setActionProblem(feedProblem(error));
    });
  }, []);
  const playVideo = (video: FeedVideo) => {
    setActionProblem(null);
    ipc
      .feedPlay('yt', video.id, strings.windowTitles.player)
      .then(() => {
        setWatched((seen) => new Set(seen).add(video.id));
      })
      .catch((error: unknown) => {
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
  const mark = useCallback(
    (channel: ChannelMark, list: MarkList | null) => {
      setActionProblem(null);
      // The list the channel was on, so Undo puts it back there.
      const same = (item: ChannelMark) => item.kind === channel.kind && item.id === channel.id;
      const before: MarkList | null = marks.favorite.some(same)
        ? 'favorite'
        : marks.hidden.some(same)
          ? 'hidden'
          : null;
      const undo = () => {
        setNotice(null);
        ipc
          .feedMark(channel, before)
          .then(setMarks)
          .catch((error: unknown) => {
            setActionProblem(feedProblem(error));
          });
      };
      ipc
        .feedMark(channel, list)
        .then((after) => {
          setMarks(after);
          setNotice(
            list === 'hidden' ? { text: t.channelHidden({ name: channel.name }), undo } : null,
          );
        })
        .catch((error: unknown) => {
          setActionProblem(feedProblem(error));
        });
    },
    [marks],
  );
  const copyLink = useCallback((url: string) => {
    setActionProblem(null);
    void navigator.clipboard
      .writeText(url)
      .then(() => {
        setNotice({ text: t.linkCopied, undo: null });
      })
      .catch(() => undefined);
  }, []);
  const channelAdded = useCallback((_place: MarkKind, name: string) => {
    setNotice({ text: t.channelAdded({ name }), undo: null });
    setAdded((count) => count + 1);
  }, []);

  const view = result?.view ?? null;
  const header = (
    <header className={styles.pageHead}>
      <h2 className={styles.pageTitle}>{t.title}</h2>
      <div className={styles.pageActions}>
        <AddChannel onAdded={channelAdded} />
        <Button
          disabled={refreshing}
          onClick={() => {
            setRefreshing(true);
            setToast(null);
            // The cards stay: what is new comes in at the top, and a note says what was found.
            void fetchFeed(true)
              .then(async (fetched) => {
                // Without a list (no feed yet) only the feed itself was asked.
                const shown = list.current;
                const added = shown === null ? (fetched ? 0 : null) : await shown.freshen();
                setToast(
                  added === null
                    ? { text: t.refreshFailed, failed: true }
                    : { text: t.refreshed({ count: added }), failed: false },
                );
              })
              .finally(() => {
                setRefreshing(false);
                setRefreshes((count) => count + 1);
              });
          }}
        >
          <Icon name="refresh" size={18} />
          {refreshing ? t.refreshing : t.refresh}
        </Button>
      </div>
    </header>
  );

  const content =
    view === null ? null : (
      <FeedParts
        snapshot={snapshot}
        feed={view.feed}
        settings={settings}
        own={own}
        ownStreams={ownStreams}
        favorites={favorites}
        marks={marks}
        onMark={mark}
        onCopyLink={copyLink}
        watched={watched}
        liveTick={liveTick}
        chosen={chosen}
        reload={`${languages} ${String(online)} ${String(settings.snapshots)} ${String(settings.shorts)} ${settings.hidden.join(',')}`}
        listRef={list}
        onFilter={setFilter}
        onOpen={setOpen}
        onLink={openLink}
        onOpenInstallations={onOpenInstallations}
        onInstallVersion={onInstallVersion}
        onPlayVideo={playVideo}
        onPlayStream={playStream}
        ads={ads}
      />
    );

  return (
    <div className={styles.screen}>
      {header}
      {view?.stale === true && (
        <Notice tone="warning">{t.stale({ date: dateTime(view.feed.generated) })}</Notice>
      )}
      {view !== null && result?.problem != null && (
        <Notice tone="warning">{t.problems[result.problem]}</Notice>
      )}
      {actionProblem !== null && <Notice tone="danger">{t.problems[actionProblem]}</Notice>}
      {notice !== null && (
        <Notice tone="info">
          <span className={styles.noticeRow}>
            <span>{notice.text}</span>
            {notice.undo !== null && (
              <Button size="small" onClick={notice.undo}>
                {t.undo}
              </Button>
            )}
          </span>
        </Notice>
      )}
      {content ??
        (result?.problem != null ? (
          <Notice tone="danger">
            <p>{t.problems[result.problem]}</p>
          </Notice>
        ) : (
          <FeedLoading />
        ))}
      {toast !== null && (
        <p
          className={[styles.toast, toast.failed && styles.toastFailed].filter(Boolean).join(' ')}
          role="status"
        >
          <Icon name={toast.failed ? 'warn' : 'check'} size={18} />
          {toast.text}
        </p>
      )}
      {open !== null && (
        <NotePanel
          key={open.id}
          card={open}
          snapshot={snapshot}
          onClose={closeNote}
          onLink={openLink}
          onOpenInstallations={onOpenInstallations}
          onInstallVersion={onInstallVersion}
        />
      )}
    </div>
  );
}

interface FeedPartsProps {
  snapshot: AppSnapshot;
  feed: Feed;
  settings: FeedSettings;
  own: FeedVideo[];
  ownStreams: FeedStream[];
  favorites: FavoriteFeed;
  marks: FeedMarks;
  onMark: (channel: ChannelMark, list: MarkList | null) => void;
  onCopyLink: (url: string) => void;
  watched: ReadonlySet<string>;
  liveTick: number;
  chosen: FeedFilter;
  /** Changes when the list should start again. */
  reload: string;
  listRef: Ref<ListHandle>;
  onFilter: (filter: FeedFilter) => void;
  onOpen: (card: NoteCard) => void;
  /** Opens an event's or announcement's link in the browser. */
  onLink: (url: string) => void;
  onOpenInstallations: (instanceId: string | null) => void;
  onInstallVersion: (versionId: string) => void;
  onPlayVideo: (video: FeedVideo) => void;
  onPlayStream: (stream: FeedStream) => void;
  ads: ServerAd[];
}

/**
 * Under All, updates, events and tips; then the filter pills and the endless
 * mosaic they choose, the newest version first.
 */
function FeedParts({
  snapshot,
  feed,
  settings,
  own,
  ownStreams,
  favorites,
  marks,
  onMark,
  onCopyLink,
  watched,
  liveTick,
  chosen,
  reload,
  listRef,
  onFilter,
  onOpen,
  onLink,
  onOpenInstallations,
  onInstallVersion,
  onPlayVideo,
  onPlayStream,
  ads,
}: FeedPartsProps) {
  const filterLabel = useId();
  const parts = visibleParts(feed, settings);
  const filters = FILTERS.filter((item) =>
    item === 'all'
      ? true
      : item === 'shorts'
        ? settings.shorts && !parts.hidden.has('videos')
        : !parts.hidden.has(item),
  );
  const filter = filters.includes(chosen) ? chosen : 'all';
  const newest = parts.versions[0] ?? null;
  const featured =
    filter === 'all' && (newest !== null || feed.latest.release !== null)
      ? { note: newest, release: feed.latest.release }
      : null;

  return (
    <>
      {filter === 'all' && <Side feed={feed} showCycle={settings.snapshots} onLink={onLink} />}
      <div className={styles.pills} role="group" aria-labelledby={filterLabel}>
        <span id={filterLabel} className={styles.visuallyHidden}>
          {t.filters}
        </span>
        {filters.map((item) => (
          <button
            key={item}
            type="button"
            className={styles.pill}
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
        ownStreams={ownStreams}
        favorites={favorites}
        marks={marks}
        onMark={onMark}
        onCopyLink={onCopyLink}
        watched={watched}
        liveTick={liveTick}
        filter={filter}
        key={`${filter} ${reload}`}
        memoryKey={`${filter} ${reload}`}
        ref={listRef}
        featured={featured}
        onOpen={onOpen}
        onOpenInstallations={onOpenInstallations}
        onInstallVersion={onInstallVersion}
        onPlayVideo={onPlayVideo}
        onPlayStream={onPlayStream}
        ads={ads}
        snapshot={snapshot}
      />
    </>
  );
}
