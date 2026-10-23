import type {
  ChannelMark,
  Feed,
  ServerAd,
  FeedBlock,
  FeedItem,
  FeedSettings,
  FeedStream,
  FeedVideo,
} from '../../lib/contracts';
import { visibleParts } from './parts';

/** What the chips show: Shorts are videos alone. */
export type FeedFilter = 'all' | FeedBlock | 'shorts';

/** The part of the feed a filter pages through; null for all. */
export function filterPart(filter: FeedFilter): FeedBlock | null {
  return filter === 'all' ? null : filter === 'shorts' ? 'videos' : filter;
}

export function itemKey(item: FeedItem): string {
  switch (item.type) {
    case 'note':
      return `note:${item.note.id}`;
    case 'video':
      return `video:${item.video.id}`;
    case 'stream':
      return `stream:${item.stream.login}`;
  }
}

function itemDate(item: FeedItem): string {
  switch (item.type) {
    case 'note':
      return item.note.date;
    case 'video':
      return item.video.date;
    case 'stream':
      return item.stream.started;
  }
}

/** Newest first; items of the same time keep their order. */
function newestFirst(items: FeedItem[]): FeedItem[] {
  return items
    .map((item, index) => ({ item, index, time: Date.parse(itemDate(item)) || 0 }))
    .sort((a, b) => b.time - a.time || a.index - b.index)
    .map(({ item }) => item);
}

export function wants(filter: FeedFilter, part: FeedBlock): boolean {
  return filter === 'all' || filterPart(filter) === part;
}

/** A video the filter shows: Shorts alone under Shorts. */
export function videoWanted(filter: FeedFilter, video: FeedVideo): boolean {
  return filter !== 'shorts' || video.short;
}

/** The feed kept by the core, as one list: shown when the pages cannot be fetched. */
export function keptItems(feed: Feed, settings: FeedSettings, filter: FeedFilter): FeedItem[] {
  const parts = visibleParts(feed, settings);
  const items: FeedItem[] = [];
  if (wants(filter, 'versions')) {
    items.push(...parts.versions.map((note): FeedItem => ({ type: 'note', note })));
  }
  if (wants(filter, 'news')) {
    items.push(...parts.news.map((note): FeedItem => ({ type: 'note', note })));
  }
  if (wants(filter, 'videos')) {
    items.push(
      ...parts.videos
        .filter((video) => videoWanted(filter, video))
        .map((video): FeedItem => ({ type: 'video', video })),
    );
  }
  if (wants(filter, 'streams')) {
    items.push(...parts.streams.map((stream): FeedItem => ({ type: 'stream', stream })));
  }
  return newestFirst(items);
}

/**
 * `extra` among `base`, each before the first item of `base` older than it.
 * The order of `base` stays as it is: the service spreads a busy channel's
 * videos, so its pages are not strictly newest first.
 */
export function mergeByTime(base: FeedItem[], extra: FeedItem[]): FeedItem[] {
  const waiting = newestFirst(extra);
  const out: FeedItem[] = [];
  let next = 0;
  for (const item of base) {
    const time = Date.parse(itemDate(item)) || 0;
    while (next < waiting.length) {
      const candidate = waiting[next];
      if (candidate === undefined || (Date.parse(itemDate(candidate)) || 0) < time) break;
      out.push(candidate);
      next += 1;
    }
    out.push(item);
  }
  return [...out, ...waiting.slice(next)];
}

/**
 * The pages fetched so far with the player's own videos and live streams
 * among them: an own item goes in once the pages have reached its time, so
 * the order holds.
 */
export function shownItems(
  pages: FeedItem[],
  own: FeedVideo[],
  ownStreams: FeedStream[],
  more: boolean,
  skip: string | null,
): FeedItem[] {
  const seen = new Set(pages.map(itemKey));
  // The oldest time the pages have reached.
  const last = pages.reduce(
    (oldest, item) => Math.min(oldest, Date.parse(itemDate(item)) || 0),
    Infinity,
  );
  const ownItems = [
    ...own.map((video): FeedItem => ({ type: 'video', video })),
    ...ownStreams.map((stream): FeedItem => ({ type: 'stream', stream })),
  ]
    .filter((item) => !seen.has(itemKey(item)))
    .filter((item) => !more || Date.parse(itemDate(item)) >= last);
  return mergeByTime(pages, ownItems).filter(
    (item) => item.type !== 'note' || item.note.id !== skip,
  );
}

/**
 * The list with its live streams as they are now: those ended leave, the
 * others are updated where they are, and with `addNew` new ones come in by
 * their time. The mixed list shows only the most watched few, so it takes
 * no new ones (they come with Refresh).
 */
export function withLiveStreams(
  items: FeedItem[],
  live: FeedStream[],
  addNew: boolean,
): FeedItem[] {
  const now = new Map(live.map((stream) => [stream.login, stream]));
  const kept = items.flatMap((item): FeedItem[] => {
    if (item.type !== 'stream') return [item];
    const stream = now.get(item.stream.login);
    return stream === undefined ? [] : [{ type: 'stream', stream }];
  });
  if (!addNew) return kept;
  const shown = new Set(kept.map(itemKey));
  return mergeByTime(
    kept,
    live
      .map((stream): FeedItem => ({ type: 'stream', stream }))
      .filter((item) => !shown.has(itemKey(item))),
  );
}

/**
 * The list after a fresh look at its first page: what it did not have comes in
 * where the page has it (before the next card the list has, so the newest go
 * first), and the cards it had are updated where they are. New live streams come
 * in only with `newStreams` (see withLiveStreams).
 */
export function withFirstPage(
  items: FeedItem[],
  first: FeedItem[],
  newStreams: boolean,
): FeedItem[] {
  const fresh = new Map(first.map((item) => [itemKey(item), item]));
  const shown = new Set(items.map(itemKey));
  // Newcomers wait for the next card of the page that the list has, and go before it.
  const before = new Map<string, FeedItem[]>();
  let waiting: FeedItem[] = [];
  let last: string | null = null;
  for (const item of first) {
    const key = itemKey(item);
    if (shown.has(key)) {
      if (waiting.length > 0) before.set(key, waiting);
      waiting = [];
      last = key;
    } else if (newStreams || item.type !== 'stream') {
      waiting.push(item);
    }
  }
  // None of the page is in the list: all of it goes first.
  const out: FeedItem[] = last === null ? [...waiting] : [];
  for (const item of items) {
    const key = itemKey(item);
    out.push(...(before.get(key) ?? []), fresh.get(key) ?? item);
    // Newcomers after the last card the list has go right after it.
    if (key === last) out.push(...waiting);
  }
  return out;
}

/**
 * The list after a fresh look: its first page (see withFirstPage) and, when
 * given, the streams live now (see withLiveStreams). On the Streams chip new
 * streams come in by their time; elsewhere only with `newStreams`.
 */
export function freshList(
  items: FeedItem[],
  first: FeedItem[],
  live: FeedStream[] | null,
  streamsChip: boolean,
  newStreams: boolean,
): FeedItem[] {
  const paged = withFirstPage(items, first, streamsChip || newStreams);
  return live === null ? paged : withLiveStreams(paged, live, streamsChip);
}

/** The keys of the items in `after` that `before` did not have, in order. */
export function newKeys(before: FeedItem[], after: FeedItem[]): string[] {
  const had = new Set(before.map(itemKey));
  return after.map(itemKey).filter((key) => !had.has(key));
}

/** The channel an item comes from: a video's YouTube channel, a stream's Twitch channel. */
export function channelOf(item: FeedItem): ChannelMark | null {
  switch (item.type) {
    case 'note':
      return null;
    case 'video':
      return { kind: 'youtube', id: item.video.channelId, name: item.video.channel };
    case 'stream':
      return { kind: 'twitch', id: item.stream.login, name: item.stream.name };
  }
}

export function onList(list: readonly ChannelMark[], channel: ChannelMark | null): boolean {
  return (
    channel !== null && list.some((mark) => mark.kind === channel.kind && mark.id === channel.id)
  );
}

/** The item's page on its site, to copy; null when it has none. */
export function itemLink(item: FeedItem): string | null {
  switch (item.type) {
    case 'note':
      return item.note.url;
    case 'video':
      return item.video.short
        ? `https://www.youtube.com/shorts/${encodeURIComponent(item.video.id)}`
        : `https://www.youtube.com/watch?v=${encodeURIComponent(item.video.id)}`;
    case 'stream':
      return `https://www.twitch.tv/${encodeURIComponent(item.stream.login)}`;
  }
}

/**
 * The list as the player marked it: the favourite channels' items first
 * (newest first), then the rest; hidden channels left out. Favourites are
 * those fetched for them up front, so a later page never moves a tile above
 * the ones already shown.
 */
export function markedItems(
  items: FeedItem[],
  favorites: FeedItem[],
  hidden: readonly ChannelMark[],
): FeedItem[] {
  const shown = (item: FeedItem) => !onList(hidden, channelOf(item));
  const first = newestFirst(favorites.filter(shown));
  const keys = new Set(first.map(itemKey));
  return [...first, ...items.filter((item) => shown(item) && !keys.has(itemKey(item)))];
}

/** The first server ad is the second tile, then one every this many (D-069). */
export const AD_FIRST = 1;
export const AD_EVERY = 16;

/** A small seeded random source, so a list keeps its order while it is shown. */
function seeded(seed: number): () => number {
  let state = Math.floor(seed * 2 ** 32) >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let x = state;
    x = Math.imul(x ^ (x >>> 15), x | 1);
    x ^= x + Math.imul(x ^ (x >>> 7), x | 61);
    return ((x ^ (x >>> 14)) >>> 0) / 2 ** 32;
  };
}

/** The ads in the order they take their places: heavier ones tend to come first. */
export function adOrder(ads: ServerAd[], seed: number): ServerAd[] {
  const random = seeded(seed);
  const left = [...ads];
  const order: ServerAd[] = [];
  while (left.length > 0) {
    const total = left.reduce((sum, ad) => sum + Math.max(1, ad.weight), 0);
    let pick = random() * total;
    let index = 0;
    for (; index < left.length - 1; index += 1) {
      pick -= Math.max(1, left[index]?.weight ?? 1);
      if (pick < 0) break;
    }
    order.push(...left.splice(index, 1));
  }
  return order;
}

export type ListEntry<T = FeedItem> =
  { type: 'item'; item: T } | { type: 'ad'; ad: ServerAd; slot: number };

/**
 * The list with server ads among its tiles: the second tile, then one every
 * `AD_EVERY`, taking the ads in turn. A list too short for a place shows none there.
 */
export function withAds<T = FeedItem>(items: T[], ads: ServerAd[]): ListEntry<T>[] {
  const entries: ListEntry<T>[] = [];
  let slot = 0;
  items.forEach((item, index) => {
    const ad = ads[slot % Math.max(1, ads.length)];
    if (ad !== undefined && index >= AD_FIRST && (index - AD_FIRST) % (AD_EVERY - 1) === 0) {
      entries.push({ type: 'ad', ad, slot });
      slot += 1;
    }
    entries.push({ type: 'item', item });
  });
  return entries;
}
