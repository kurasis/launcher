import type {
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
  const lastItem = pages.at(-1);
  const last = lastItem === undefined ? Infinity : Date.parse(itemDate(lastItem));
  const ownItems = [
    ...own.map((video): FeedItem => ({ type: 'video', video })),
    ...ownStreams.map((stream): FeedItem => ({ type: 'stream', stream })),
  ]
    .filter((item) => !seen.has(itemKey(item)))
    .filter((item) => !more || Date.parse(itemDate(item)) >= last);
  return newestFirst([...pages, ...ownItems]).filter(
    (item) => item.type !== 'note' || item.note.id !== skip,
  );
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

export type ListEntry =
  { type: 'item'; item: FeedItem } | { type: 'ad'; ad: ServerAd; slot: number };

/**
 * The list with server ads among its tiles: the second tile, then one every
 * `AD_EVERY`, taking the ads in turn. A list too short for a place shows none there.
 */
export function withAds(items: FeedItem[], ads: ServerAd[]): ListEntry[] {
  const entries: ListEntry[] = [];
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
