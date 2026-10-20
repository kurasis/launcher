import type { Feed, FeedBlock, FeedItem, FeedSettings, FeedVideo } from '../../lib/contracts';
import { visibleParts } from './parts';

/** What the chips show. */
export type FeedFilter = 'all' | FeedBlock;

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
  return filter === 'all' || filter === part;
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
    items.push(...parts.videos.map((video): FeedItem => ({ type: 'video', video })));
  }
  if (wants(filter, 'streams')) {
    items.push(...parts.streams.map((stream): FeedItem => ({ type: 'stream', stream })));
  }
  return newestFirst(items);
}

/**
 * The pages fetched so far with the player's own videos among them: an own
 * video goes in once the pages have reached its time, so the order holds.
 */
export function shownItems(
  pages: FeedItem[],
  own: FeedVideo[],
  more: boolean,
  skip: string | null,
): FeedItem[] {
  const seen = new Set(pages.map(itemKey));
  const lastItem = pages.at(-1);
  const last = lastItem === undefined ? Infinity : Date.parse(itemDate(lastItem));
  const ownItems = own
    .map((video): FeedItem => ({ type: 'video', video }))
    .filter((item) => !seen.has(itemKey(item)))
    .filter((item) => !more || Date.parse(itemDate(item)) >= last);
  return newestFirst([...pages, ...ownItems]).filter(
    (item) => item.type !== 'note' || item.note.id !== skip,
  );
}
