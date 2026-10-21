import { type ReactNode, useCallback, useEffect, useRef, useState } from 'react';
import { Button } from '../../components/Button';
import { StatusBadge } from '../../components/StatusBadge';
import type {
  Feed,
  FeedItem,
  FeedSettings,
  FeedStream,
  FeedVideo,
  NoteCard,
} from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import {
  type FeedFilter,
  filterPart,
  itemKey,
  keptItems,
  shownItems,
  videoWanted,
  wants,
} from './list';
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
}) {
  return (
    <article className={styles.tile}>
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

function NoteItem({ card, onOpen }: { card: NoteCard; onOpen: (card: NoteCard) => void }) {
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
    />
  );
}

function VideoItem({ video, onPlay }: { video: FeedVideo; onPlay: (video: FeedVideo) => void }) {
  return (
    <Tile
      image={pictureUrl(video.image)}
      cover={video.channel}
      overlay={video.short ? { tone: 'neutral', label: t.short } : null}
      badges={null}
      title={video.title}
      label={t.watchVideo({ title: video.title, channel: video.channel })}
      line={`${video.channel} · ${relativeTime(video.date)}`}
      onClick={() => {
        onPlay(video);
      }}
    />
  );
}

function StreamItem({
  stream,
  onPlay,
}: {
  stream: FeedStream;
  onPlay: (stream: FeedStream) => void;
}) {
  const viewers = t.viewers({ count: stream.viewers });
  return (
    <Tile
      image={pictureUrl(stream.image)}
      cover={stream.name}
      overlay={{ tone: 'danger', label: t.live }}
      badges={null}
      title={stream.title !== '' ? stream.title : stream.name}
      label={t.watchStream({ name: stream.name, title: stream.title, viewers })}
      line={`${stream.name} · ${viewers}`}
      onClick={() => {
        onPlay(stream);
      }}
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
  filter: FeedFilter;
  /** A note shown above the list already. */
  skip: string | null;
  onOpen: (card: NoteCard) => void;
  onPlayVideo: (video: FeedVideo) => void;
  onPlayStream: (stream: FeedStream) => void;
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
  filter,
  skip,
  onOpen,
  onPlayVideo,
  onPlayStream,
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

  const ownShown =
    wants(filter, 'videos') && !settings.hidden.includes('videos')
      ? own.filter((video) => (settings.shorts || !video.short) && videoWanted(filter, video))
      : [];
  const streamsShown =
    wants(filter, 'streams') && !settings.hidden.includes('streams') ? ownStreams : [];
  const items = shownItems(pages, ownShown, streamsShown, next !== null && !kept, skip);
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
          {items.map((item) => (
            <li key={itemKey(item)}>
              {item.type === 'note' ? (
                <NoteItem card={item.note} onOpen={onOpen} />
              ) : item.type === 'video' ? (
                <VideoItem video={item.video} onPlay={onPlayVideo} />
              ) : (
                <StreamItem stream={item.stream} onPlay={onPlayStream} />
              )}
            </li>
          ))}
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
