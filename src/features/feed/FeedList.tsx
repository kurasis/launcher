import { type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Button } from '../../components/Button';
import { StatusBadge } from '../../components/StatusBadge';
import { type MenuItem, MoreMenu } from '../../app/MoreMenu';
import type {
  AppSnapshot,
  ChannelMark,
  FavoriteFeed,
  Feed,
  FeedItem,
  FeedSettings,
  FeedStream,
  FeedMarks,
  FeedVideo,
  MarkList,
  NoteCard,
  ServerAd,
} from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import {
  adOrder,
  channelOf,
  type FeedFilter,
  filterPart,
  itemKey,
  itemLink,
  keptItems,
  markedItems,
  onList,
  shownItems,
  videoWanted,
  wants,
  withAds,
  withLiveStreams,
} from './list';
import { ServerAdTile } from './ServerAdTile';
import { kindLabel } from './parts';
import { pictureUrl } from './pictureUrl';
import { relativeTime, shortDate } from './when';
import styles from './Feed.module.css';

const t = strings.feed;

/** How far below the screen the next page is asked for. */
const AHEAD = '800px';

type Tone = 'neutral' | 'success' | 'warning' | 'danger';

/**
 * One tile of the grid: a picture (or, without one, a plain cover with
 * `cover` on it), badges, the title and a line under it. The title's button
 * covers the whole tile.
 */
function Tile({
  image,
  cover,
  overlay,
  badges,
  title,
  label,
  line,
  onClick,
  menu,
  dimmed = false,
}: {
  image: string | null;
  cover: string;
  overlay: { tone: Tone; label: string } | null;
  badges: ReactNode;
  title: string;
  /** The button's name when the title alone does not say what it does. */
  label?: string;
  line: string;
  onClick: () => void;
  /** The ⋯ menu in the corner. */
  menu: ReactNode;
  /** Seen already: shown fainter. */
  dimmed?: boolean;
}) {
  return (
    <article className={dimmed ? [styles.tile, styles.watched].join(' ') : styles.tile}>
      {menu}
      <span className={styles.preview}>
        {image !== null ? (
          <img src={image} alt="" loading="lazy" />
        ) : (
          <span className={styles.cover} dir="auto">
            {cover}
          </span>
        )}
        {overlay !== null && (
          <span className={styles.overlayBadge}>
            <StatusBadge tone={overlay.tone} label={overlay.label} />
          </span>
        )}
      </span>
      <div className={styles.tileText}>
        {badges !== null && <div className={styles.badges}>{badges}</div>}
        <h3 className={styles.tileTitle}>
          <button type="button" className={styles.cardButton} aria-label={label} onClick={onClick}>
            {title}
          </button>
        </h3>
        {line !== '' && <p className={styles.tileLine}>{line}</p>}
      </div>
    </article>
  );
}

/** The streams live now, as the service lists them (two pages at most). */
async function liveStreams(): Promise<FeedStream[]> {
  const first = await ipc.feedPage('streams', null);
  const second = first.next === null ? null : await ipc.feedPage('streams', first.next);
  return [...first.items, ...(second?.items ?? [])].flatMap((item) =>
    item.type === 'stream' ? [item.stream] : [],
  );
}

/** What a tile's ⋯ menu does. */
interface ItemActions {
  marks: FeedMarks;
  onMark: (channel: ChannelMark, list: MarkList | null) => void;
  onCopyLink: (url: string) => void;
  watched: ReadonlySet<string>;
}

/** The ⋯ menu of a tile: copy its link, show its channel first or not at all. */
function ItemMenu({
  item,
  title,
  actions,
}: {
  item: FeedItem;
  title: string;
  actions: ItemActions;
}) {
  const channel = channelOf(item);
  const link = itemLink(item);
  const entries: MenuItem[] = [];
  if (link !== null) {
    entries.push({
      kind: 'item',
      key: 'copy',
      label: t.copyLink,
      onSelect: () => {
        actions.onCopyLink(link);
      },
    });
  }
  if (channel !== null) {
    const favorite = onList(actions.marks.favorite, channel);
    entries.push(
      {
        kind: 'item',
        key: 'favorite',
        label: favorite ? t.unfavorite({ name: channel.name }) : t.favorite({ name: channel.name }),
        onSelect: () => {
          actions.onMark(channel, favorite ? null : 'favorite');
        },
      },
      {
        kind: 'item',
        key: 'hide',
        label: t.hideChannel({ name: channel.name }),
        onSelect: () => {
          actions.onMark(channel, 'hidden');
        },
      },
    );
  }
  if (entries.length === 0) return null;
  return (
    <span className={styles.tileMenu}>
      <MoreMenu
        label="⋯"
        ariaLabel={t.itemMenu({ title })}
        buttonClassName={styles.tileMenuButton}
        caret={false}
        current={false}
        entries={entries}
      />
    </span>
  );
}

function NoteItem({
  card,
  onOpen,
  actions,
}: {
  card: NoteCard;
  onOpen: (card: NoteCard) => void;
  actions: ItemActions;
}) {
  return (
    <Tile
      image={pictureUrl(card.image)}
      cover={card.version ?? kindLabel(card)}
      overlay={null}
      badges={
        <>
          <StatusBadge tone={card.kind === null ? 'neutral' : 'success'} label={kindLabel(card)} />
          <span className={styles.muted}>{shortDate(card.date)}</span>
        </>
      }
      title={card.title}
      line={card.summary}
      onClick={() => {
        onOpen(card);
      }}
      menu={<ItemMenu item={{ type: 'note', note: card }} title={card.title} actions={actions} />}
    />
  );
}

function VideoItem({
  video,
  onPlay,
  actions,
}: {
  video: FeedVideo;
  onPlay: (video: FeedVideo) => void;
  actions: ItemActions;
}) {
  const item: FeedItem = { type: 'video', video };
  const favorite = onList(actions.marks.favorite, channelOf(item));
  const watched = actions.watched.has(video.id);
  return (
    <Tile
      image={pictureUrl(video.image)}
      cover={video.channel}
      overlay={video.short ? { tone: 'neutral', label: t.short } : null}
      badges={
        favorite || watched ? (
          <>
            {favorite && <StatusBadge tone="warning" label={t.favoriteBadge} />}
            {watched && <StatusBadge tone="neutral" label={t.watched} />}
          </>
        ) : null
      }
      dimmed={watched}
      title={video.title}
      label={t.watchVideo({ title: video.title, channel: video.channel })}
      line={`${video.channel} · ${relativeTime(video.date)}`}
      onClick={() => {
        onPlay(video);
      }}
      menu={<ItemMenu item={item} title={video.title} actions={actions} />}
    />
  );
}

function StreamItem({
  stream,
  onPlay,
  actions,
}: {
  stream: FeedStream;
  onPlay: (stream: FeedStream) => void;
  actions: ItemActions;
}) {
  const viewers = t.viewers({ count: stream.viewers });
  const item: FeedItem = { type: 'stream', stream };
  const favorite = onList(actions.marks.favorite, channelOf(item));
  const title = stream.title !== '' ? stream.title : stream.name;
  return (
    <Tile
      image={pictureUrl(stream.image)}
      cover={stream.name}
      overlay={{ tone: 'danger', label: t.live }}
      badges={favorite ? <StatusBadge tone="warning" label={t.favoriteBadge} /> : null}
      title={title}
      label={t.watchStream({ name: stream.name, title: stream.title, viewers })}
      line={`${stream.name} · ${viewers}`}
      onClick={() => {
        onPlay(stream);
      }}
      menu={<ItemMenu item={item} title={title} actions={actions} />}
    />
  );
}

interface FeedListProps {
  /** The feed the core has: shown as one list when the pages cannot be fetched. */
  feed: Feed;
  settings: FeedSettings;
  own: FeedVideo[];
  /** The live streams of the player's own Twitch channels. */
  ownStreams: FeedStream[];
  /** The favourite channels' videos and streams, shown first. */
  favorites: FavoriteFeed;
  /** The channels the player hid or put first. */
  marks: FeedMarks;
  onMark: (channel: ChannelMark, list: MarkList | null) => void;
  onCopyLink: (url: string) => void;
  /** Videos the player opened from the feed: shown dimmed. */
  watched: ReadonlySet<string>;
  /** Changes every few minutes: the live streams are asked again. */
  liveTick: number;
  filter: FeedFilter;
  /** A note shown above the list already. */
  skip: string | null;
  onOpen: (card: NoteCard) => void;
  onPlayVideo: (video: FeedVideo) => void;
  onPlayStream: (stream: FeedStream) => void;
  /** Server ads showing now (D-069); they go among the tiles of "All". */
  ads?: ServerAd[];
  snapshot?: AppSnapshot;
}

type Loading = 'first' | 'more' | 'idle' | 'failed';

/**
 * The endless list (D-067): patch notes, news, videos and streams mixed,
 * newest first, as a grid of tiles, several to a row. The next page is asked for as the end
 * of the list comes near. Give it a key that changes when the list should
 * start again (the part, languages, settings, Refresh).
 */
export function FeedList({
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
  filter,
  skip,
  onOpen,
  onPlayVideo,
  onPlayStream,
  ads = [],
  snapshot,
}: FeedListProps) {
  const [pages, setPages] = useState<FeedItem[]>([]);
  const [next, setNext] = useState<string | null>(null);
  const [loading, setLoading] = useState<Loading>('first');
  /** The pages could not be fetched: the feed the core has is shown. */
  const [kept, setKept] = useState(false);
  // Answers that come after the list is gone are dropped.
  const alive = useRef(true);
  const end = useRef<HTMLDivElement>(null);
  const part = filterPart(filter);
  const shortsOnly = filter === 'shorts';
  // What the kept list is made from when the pages fail; the list starts
  // again only when its key changes, not when these do.
  const feedRef = useRef(feed);
  const settingsRef = useRef(settings);
  useEffect(() => {
    feedRef.current = feed;
    settingsRef.current = settings;
  });

  useEffect(() => {
    alive.current = true;
    ipc
      .feedPage(part, null, shortsOnly)
      .then((page) => {
        if (!alive.current) return;
        setPages(page.items);
        setNext(page.next);
        setLoading('idle');
      })
      .catch(() => {
        if (!alive.current) return;
        setPages(keptItems(feedRef.current, settingsRef.current, filter));
        setKept(true);
        setLoading('idle');
      });
    return () => {
      alive.current = false;
    };
  }, [part, filter, shortsOnly]);

  // Live streams change while the list is shown: each tick asks for them
  // again, so ended ones leave and, under the Streams chip, new ones come in.
  const liveShown = wants(filter, 'streams') && !settings.hidden.includes('streams') && !kept;
  const tickSeen = useRef(liveTick);
  useEffect(() => {
    if (liveTick === tickSeen.current) return;
    tickSeen.current = liveTick;
    if (!liveShown) return;
    liveStreams()
      .then((live) => {
        if (alive.current) {
          setPages((before) => withLiveStreams(before, live, filter === 'streams'));
        }
      })
      .catch(() => undefined);
  }, [liveTick, liveShown, filter]);

  const loadMore = useCallback(() => {
    if (next === null || (loading !== 'idle' && loading !== 'failed')) return;
    setLoading('more');
    ipc
      .feedPage(part, next, shortsOnly)
      .then((page) => {
        if (!alive.current) return;
        setPages((before) => {
          const seen = new Set(before.map(itemKey));
          return [...before, ...page.items.filter((item) => !seen.has(itemKey(item)))];
        });
        setNext(page.next);
        setLoading('idle');
      })
      .catch(() => {
        if (alive.current) setLoading('failed');
      });
  }, [next, loading, part, shortsOnly]);

  const watching = typeof IntersectionObserver !== 'undefined';
  useEffect(() => {
    const target = end.current;
    if (!watching || target === null || next === null || loading !== 'idle') return;
    // Looked up again here: the global can be gone by the time the effect runs (a test
    // that stubbed it has put it back while the list is still mounted).
    if (typeof IntersectionObserver === 'undefined') return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) loadMore();
      },
      { rootMargin: `0px 0px ${AHEAD} 0px` },
    );
    observer.observe(target);
    return () => {
      observer.disconnect();
    };
  }, [watching, next, loading, loadMore]);

  const videosWanted = wants(filter, 'videos') && !settings.hidden.includes('videos');
  const streamsWanted = wants(filter, 'streams') && !settings.hidden.includes('streams');
  const videosShown = (videos: FeedVideo[]) =>
    videosWanted
      ? videos.filter((video) => (settings.shorts || !video.short) && videoWanted(filter, video))
      : [];
  // Only channels still marked: a favourite taken off leaves the top at once.
  const favoriteItems: FeedItem[] = [
    ...videosShown(favorites.videos).map((video): FeedItem => ({ type: 'video', video })),
    ...(streamsWanted ? favorites.streams : []).map((stream): FeedItem => ({
      type: 'stream',
      stream,
    })),
  ].filter((item) => onList(marks.favorite, channelOf(item)));
  const items = markedItems(
    shownItems(
      pages,
      videosShown(own),
      streamsWanted ? ownStreams : [],
      next !== null && !kept,
      skip,
    ),
    favoriteItems,
    marks.hidden,
  );
  const actions: ItemActions = { marks, onMark, onCopyLink, watched };
  // The order ads take their places in stays while this list is shown.
  const [seed] = useState(Math.random);
  const ordered = useMemo(() => adOrder(ads, seed), [ads, seed]);
  const entries = withAds(items, filter === 'all' && snapshot !== undefined ? ordered : []);
  const empty =
    filter === 'videos' || filter === 'shorts'
      ? t.noVideos
      : filter === 'streams'
        ? t.noStreams
        : filter === 'all'
          ? t.nothing
          : t.noArticles;

  if (loading === 'first') return <p role="status">{t.loading}</p>;
  return (
    <>
      {items.length === 0 ? (
        <p className={styles.muted}>{empty}</p>
      ) : (
        <ul className={styles.list} aria-label={t.filter[filter]}>
          {entries.map((entry) =>
            entry.type === 'ad' ? (
              snapshot !== undefined && (
                <li key={`ad:${String(entry.slot)}:${entry.ad.id}`}>
                  <ServerAdTile ad={entry.ad} snapshot={snapshot} />
                </li>
              )
            ) : (
              <li key={itemKey(entry.item)}>
                {entry.item.type === 'note' ? (
                  <NoteItem card={entry.item.note} onOpen={onOpen} actions={actions} />
                ) : entry.item.type === 'video' ? (
                  <VideoItem video={entry.item.video} onPlay={onPlayVideo} actions={actions} />
                ) : (
                  <StreamItem stream={entry.item.stream} onPlay={onPlayStream} actions={actions} />
                )}
              </li>
            ),
          )}
        </ul>
      )}
      <div ref={end} className={styles.listEnd}>
        {loading === 'more' && <p role="status">{t.loadingMore}</p>}
        {loading === 'failed' && (
          <>
            <p className={styles.muted}>{t.moreFailed}</p>
            <Button onClick={loadMore}>{t.tryAgain}</Button>
          </>
        )}
        {loading === 'idle' && next !== null && !kept && !watching && (
          <Button onClick={loadMore}>{t.loadMore}</Button>
        )}
        {loading === 'idle' && next === null && items.length > 0 && (
          <p className={styles.muted}>{t.end}</p>
        )}
      </div>
    </>
  );
}
