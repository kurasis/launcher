import type { Feed, FeedBlock, FeedSettings, NoteCard } from '../../lib/contracts';
import { strings } from '../../lib/strings';

/** The kind of a card, as its badge says it. */
export function kindLabel(card: NoteCard): string {
  return card.kind === null ? strings.feed.kinds.news : strings.feed.kinds[card.kind];
}

/** The feed's parts as these settings show them. */
export function visibleParts(feed: Feed, settings: FeedSettings) {
  const hidden = new Set<FeedBlock>(settings.hidden);
  const versions = hidden.has('versions')
    ? []
    : feed.versions.filter(
        (card) => settings.snapshots || card.kind === null || card.kind === 'release',
      );
  const news = hidden.has('news') ? [] : feed.news;
  const videos = hidden.has('videos')
    ? []
    : feed.videos.filter((video) => settings.shorts || !video.short);
  const streams = hidden.has('streams') ? [] : feed.streams;
  return { hidden, versions, news, videos, streams };
}
