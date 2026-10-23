import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Button } from '../../components/Button';
import { Icon } from '../../components/Icon';
import type { MenuItem } from '../../app/MoreMenu';
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
  LatestVersion,
  MarkList,
  NoteCard,
  ServerAd,
} from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import {
  Avatar,
  Card,
  CardMenu,
  Cell,
  KindChip,
  type KindTone,
  Mosaic,
  Picture,
  PlayCircle,
  Skeletons,
} from './Card';
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
import { ASPECT, pictureAspect } from './mosaic';
import { ServerAdTile } from './ServerAdTile';
import { kindLabel } from './parts';
import { pictureUrl } from './pictureUrl';
import { VersionAction } from './VersionAction';
import { cardDate, count, liveNow, videoLength } from './when';
import styles from './Feed.module.css';

const t = strings.feed;

/** How far below the screen the next page is asked for. */
const AHEAD = '800px';

/** The source of patch notes and news: brand names, the same in every language. */
const MINECRAFT = 'Minecraft';
const MINECRAFT_NET = 'Minecraft.net';

/** A picture's box when there is no picture: the version or the kind, on a plain cover. */
function Cover({ text }: { text: string }) {
  return (
    <span className={styles.cover} dir="auto">
      {text}
    </span>
  );
}

/** The source of a card: its picture or letter, and its name. */
function Source({
  avatar,
  name,
  favorite = false,
  square = false,
}: {
  avatar: string | null;
  name: string;
  favorite?: boolean;
  square?: boolean;
}) {
  return (
    <span className={styles.source}>
      <Avatar key={avatar ?? ''} src={avatar} name={name} square={square} />
      {favorite && (
        <>
          <Icon name="star" size={14} className={styles.star} />
          <span className={styles.visuallyHidden}>{t.favoriteBadge}</span>
        </>
      )}
      <bdi className={styles.sourceName}>{name}</bdi>
    </span>
  );
}

function noteTone(card: NoteCard): KindTone {
  return card.kind === null ? 'neutral' : card.kind === 'release' ? 'success' : 'warning';
}

/** The meta line of a patch note or a news item. */
function NoteMeta({ card }: { card: NoteCard }) {
  const source = card.kind === null ? MINECRAFT_NET : MINECRAFT;
  return (
    <>
      <Source avatar={null} name={source} />
      <KindChip tone={noteTone(card)} label={kindLabel(card)} />
      <span className={styles.date}>{cardDate(card.date)}</span>
    </>
  );
}

/** The version on a picture, at its top start. */
function VersionBadge({ version }: { version: string }) {
  return (
    <span className={[styles.chip, styles.chipVersion, styles.topStart].join(' ')} dir="ltr">
      {version}
    </span>
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

/** What a card's ⋯ menu does. */
interface ItemActions {
  marks: FeedMarks;
  onMark: (channel: ChannelMark, list: MarkList | null) => void;
  onCopyLink: (url: string) => void;
  watched: ReadonlySet<string>;
}

/** The ⋯ menu of a card: copy its link, show its channel first or not at all. */
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
  return <CardMenu label={t.itemMenu({ title })} entries={entries} />;
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
  const image = pictureUrl(card.image);
  return (
    <Card
      aspect={image === null ? ASPECT.wide : pictureAspect(card.imageWidth, card.imageHeight)}
      media={
        image !== null ? <Picture src={image} /> : <Cover text={card.version ?? kindLabel(card)} />
      }
      title={card.title}
      onClick={() => {
        onOpen(card);
      }}
      meta={<NoteMeta card={card} />}
      menu={<ItemMenu item={{ type: 'note', note: card }} title={card.title} actions={actions} />}
    />
  );
}

/**
 * The newest version, first in the mosaic and taller (the old hero): its patch note
 * opens from the card, and Play or Install for it sits under the title. Without a
 * note it is the newest release alone.
 */
function VersionCard({
  featured,
  snapshot,
  onOpen,
  onOpenInstallations,
  onInstallVersion,
}: {
  featured: Featured;
  snapshot: AppSnapshot;
  onOpen: (card: NoteCard) => void;
  onOpenInstallations: (instanceId: string | null) => void;
  onInstallVersion: (versionId: string) => void;
}) {
  const { note, release } = featured;
  const version = note !== null ? note.version : (release?.id ?? null);
  const image = pictureUrl(note?.image ?? null);
  const title = note?.title ?? t.versionTitle({ version: release?.id ?? '' });
  const kind: NoteCard = note ?? {
    id: '',
    title,
    date: release?.date ?? '',
    summary: '',
    translated: false,
    version,
    kind: 'release',
    image: null,
    imageWidth: null,
    imageHeight: null,
    url: null,
  };
  return (
    <Card
      aspect={ASPECT.tall}
      media={
        <>
          {image !== null ? <Picture src={image} /> : <Cover text={version ?? kindLabel(kind)} />}
          {/* Without a picture the cover says the version already. */}
          {version !== null && image !== null && <VersionBadge version={version} />}
        </>
      }
      title={title}
      onClick={
        note === null
          ? null
          : () => {
              onOpen(note);
            }
      }
      meta={<NoteMeta card={kind} />}
      extra={
        version !== null && (
          <VersionAction
            snapshot={snapshot}
            version={version}
            onOpenInstallations={onOpenInstallations}
            onInstallVersion={onInstallVersion}
          />
        )
      }
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
  // A Short stands upright: its own upright picture, else the wide one cropped.
  const image = video.short
    ? (pictureUrl(video.imageTall) ?? pictureUrl(video.image))
    : pictureUrl(video.image);
  const length = video.short ? null : videoLength(video.duration);
  return (
    <Card
      aspect={video.short ? ASPECT.short : ASPECT.wide}
      media={
        <>
          {image !== null ? <Picture src={image} /> : <Cover text={video.channel} />}
          <PlayCircle small={video.short} />
          {video.short && (
            <span className={[styles.chip, styles.bottomStart].join(' ')} aria-hidden="true">
              {t.short}
            </span>
          )}
          {length !== null && (
            <span
              className={[styles.chip, styles.bottomEnd].join(' ')}
              dir="ltr"
              aria-hidden="true"
            >
              {length}
            </span>
          )}
        </>
      }
      dimmed={watched}
      title={video.title}
      label={
        length === null
          ? t.watchVideo({ title: video.title, channel: video.channel })
          : t.watchVideoLength({ title: video.title, channel: video.channel, length })
      }
      onClick={() => {
        onPlay(video);
      }}
      meta={
        <>
          <Source avatar={pictureUrl(video.avatar)} name={video.channel} favorite={favorite} />
          <KindChip tone="neutral" label={video.short ? t.short : t.kinds.video} />
          <span className={styles.date}>{cardDate(video.date)}</span>
          {watched && <span className={styles.visuallyHidden}>{t.watched}</span>}
        </>
      }
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
  const image = pictureUrl(stream.image);
  return (
    <Card
      aspect={ASPECT.wide}
      media={
        <>
          {image !== null ? <Picture src={image} /> : <Cover text={stream.name} />}
          <span
            className={[styles.chip, styles.chipLive, styles.topStart].join(' ')}
            aria-hidden="true"
          >
            <Icon name="live" size={14} />
            {t.live}
          </span>
          <span className={[styles.chip, styles.bottomStart].join(' ')} aria-hidden="true">
            <Icon name="eye" size={14} />
            <bdi>{count(stream.viewers)}</bdi>
          </span>
        </>
      }
      title={title}
      label={t.watchStream({ name: stream.name, title: stream.title, viewers })}
      onClick={() => {
        onPlay(stream);
      }}
      meta={
        <>
          <Source avatar={pictureUrl(stream.avatar)} name={stream.name} favorite={favorite} />
          <KindChip tone="danger" label={t.kinds.stream} />
          <span className={styles.date}>{liveNow()}</span>
        </>
      }
      menu={<ItemMenu item={item} title={title} actions={actions} />}
    />
  );
}

/** The newest version shown first in the mosaic: its note, or the newest release alone. */
export interface Featured {
  note: NoteCard | null;
  release: LatestVersion | null;
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
  /** The newest version, first and taller; it is not repeated further down. */
  featured?: Featured | null;
  onOpenInstallations?: (instanceId: string | null) => void;
  onInstallVersion?: (versionId: string) => void;
  onOpen: (card: NoteCard) => void;
  onPlayVideo: (video: FeedVideo) => void;
  onPlayStream: (stream: FeedStream) => void;
  /** Server ads showing now (D-069); they go among the tiles of "All". */
  ads?: ServerAd[];
  snapshot?: AppSnapshot;
}

type Loading = 'first' | 'more' | 'idle' | 'failed';

/** The feed's first load: grey cards where the mosaic is coming, and the words for screen readers. */
export function FeedLoading() {
  return (
    <>
      <p role="status" className={styles.visuallyHidden}>
        {t.loading}
      </p>
      <Mosaic busy>
        <Skeletons rows={3} tall={[0, 4, 8]} />
      </Mosaic>
    </>
  );
}

function ignore() {
  // Nothing to open: the list was given no way to.
}

/**
 * The endless list (D-067): patch notes, news, videos and streams mixed,
 * newest first, as a mosaic of cards. The next page is asked for as the end
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
  featured = null,
  onOpenInstallations = ignore,
  onInstallVersion = ignore,
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
      featured?.note?.id ?? null,
    ),
    favoriteItems,
    marks.hidden,
  );
  const actions: ItemActions = { marks, onMark, onCopyLink, watched };
  // The order ads take their places in stays while this list is shown.
  const [seed] = useState(Math.random);
  const ordered = useMemo(() => adOrder(ads, seed), [ads, seed]);
  // The newest version takes the first place, so an ad is still the second card.
  const entries = withAds<FeedItem | null>(
    featured !== null && snapshot !== undefined ? [null, ...items] : items,
    filter === 'all' && snapshot !== undefined ? ordered : [],
  );
  const empty =
    filter === 'videos' || filter === 'shorts'
      ? t.noVideos
      : filter === 'streams'
        ? t.noStreams
        : filter === 'all'
          ? t.nothing
          : t.noArticles;

  if (loading === 'first') return <FeedLoading />;
  const cards = (entries.length > 0 || featured !== null) && (
    <Mosaic label={t.filter[filter]} busy={loading === 'more'}>
      {featured !== null && snapshot !== undefined && (
        <Cell key="featured">
          <VersionCard
            featured={featured}
            snapshot={snapshot}
            onOpen={onOpen}
            onOpenInstallations={onOpenInstallations}
            onInstallVersion={onInstallVersion}
          />
        </Cell>
      )}
      {entries.map((entry) =>
        entry.type === 'ad' ? (
          snapshot !== undefined && (
            <Cell key={`ad:${String(entry.slot)}:${entry.ad.id}`}>
              <ServerAdTile ad={entry.ad} snapshot={snapshot} />
            </Cell>
          )
        ) : entry.item === null ? null : (
          <Cell key={itemKey(entry.item)}>
            {entry.item.type === 'note' ? (
              <NoteItem card={entry.item.note} onOpen={onOpen} actions={actions} />
            ) : entry.item.type === 'video' ? (
              <VideoItem video={entry.item.video} onPlay={onPlayVideo} actions={actions} />
            ) : (
              <StreamItem stream={entry.item.stream} onPlay={onPlayStream} actions={actions} />
            )}
          </Cell>
        ),
      )}
      {loading === 'more' && <Skeletons rows={1} />}
    </Mosaic>
  );
  return (
    <>
      {cards}
      {items.length === 0 && <p className={styles.muted}>{empty}</p>}
      <div ref={end} className={styles.listEnd}>
        {loading === 'more' && (
          <p role="status" className={styles.loadingMore}>
            <span className={styles.spinner} aria-hidden="true" />
            {t.loadingMore}
          </p>
        )}
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
