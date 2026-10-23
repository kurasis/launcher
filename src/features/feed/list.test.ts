import { describe, expect, it } from 'vitest';
import type { FeedItem, FeedStream } from '../../lib/contracts';
import { freshList, itemKey, mergeByTime, newKeys, withFirstPage, withLiveStreams } from './list';

function video(id: string, channel: string, date: string): FeedItem {
  return {
    type: 'video',
    video: {
      id,
      channelId: `UC${channel.padEnd(22, 'x')}`,
      channel,
      title: id,
      date,
      short: false,
      image: null,
      imageTall: null,
      avatar: null,
      duration: null,
    },
  };
}

function stream(login: string, started: string, viewers = 10): FeedStream {
  return {
    login,
    name: login,
    title: `${login} live`,
    viewers,
    started,
    image: null,
    avatar: null,
  };
}

const keys = (items: FeedItem[]) => items.map(itemKey);

describe('mergeByTime', () => {
  it('keeps the order the service gave, spread channels and all', () => {
    // The service moved b1 up past a2 so one channel does not fill the list.
    const base = [
      video('a1', 'a', '2026-10-21T10:00:00Z'),
      video('b1', 'b', '2026-10-21T08:00:00Z'),
      video('a2', 'a', '2026-10-21T09:00:00Z'),
      video('a3', 'a', '2026-10-21T07:00:00Z'),
    ];
    const extra = [video('own', 'c', '2026-10-21T08:30:00Z')];
    expect(keys(mergeByTime(base, extra))).toEqual([
      'video:a1',
      'video:own',
      'video:b1',
      'video:a2',
      'video:a3',
    ]);
    expect(keys(mergeByTime(base, []))).toEqual(keys(base));
  });

  it('puts what is older than everything at the end, newest first', () => {
    const base = [video('a1', 'a', '2026-10-21T10:00:00Z')];
    const extra = [
      video('old', 'c', '2026-01-01T00:00:00Z'),
      video('older', 'c', '2025-01-01T00:00:00Z'),
      video('new', 'c', '2026-10-22T00:00:00Z'),
    ];
    expect(keys(mergeByTime(base, extra))).toEqual([
      'video:new',
      'video:a1',
      'video:old',
      'video:older',
    ]);
  });
});

describe('withLiveStreams', () => {
  it('drops the streams that ended, updates the others and can add new ones by time', () => {
    const items: FeedItem[] = [
      { type: 'stream', stream: stream('ended', '2026-10-21T11:00:00Z') },
      { type: 'stream', stream: stream('going', '2026-10-21T09:30:00Z', 5) },
      video('v1', 'a', '2026-10-21T09:00:00Z'),
      video('v2', 'a', '2026-10-21T07:00:00Z'),
    ];
    const live = [
      stream('going', '2026-10-21T09:30:00Z', 500),
      stream('fresh', '2026-10-21T08:00:00Z'),
    ];
    const after = withLiveStreams(items, live, true);
    expect(keys(after)).toEqual(['stream:going', 'video:v1', 'stream:fresh', 'video:v2']);
    const going = after[0];
    expect(going?.type === 'stream' && going.stream.viewers).toBe(500);
    // The mixed list takes no new ones.
    expect(keys(withLiveStreams(items, live, false))).toEqual([
      'stream:going',
      'video:v1',
      'video:v2',
    ]);
    // With nothing live, the other items stay as they were.
    expect(keys(withLiveStreams(items, [], true))).toEqual(['video:v1', 'video:v2']);
  });
});

describe('withFirstPage', () => {
  const shown = [
    video('v3', 'a', '2026-10-21T10:00:00Z'),
    video('v2', 'a', '2026-10-21T09:00:00Z'),
    video('v1', 'a', '2026-10-21T08:00:00Z'),
  ];

  it('puts new cards first and updates the ones it has where they are', () => {
    const renamed = video('v3', 'a', '2026-10-21T10:00:00Z');
    if (renamed.type === 'video') renamed.video.title = 'v3, renamed';
    const first = [
      video('n2', 'b', '2026-10-21T12:00:00Z'),
      video('n1', 'b', '2026-10-21T11:00:00Z'),
      renamed,
      video('v2', 'a', '2026-10-21T09:00:00Z'),
    ];
    const after = withFirstPage(shown, first, false);
    expect(keys(after)).toEqual(['video:n2', 'video:n1', 'video:v3', 'video:v2', 'video:v1']);
    const v3 = after[2];
    expect(v3?.type === 'video' && v3.video.title).toBe('v3, renamed');
    expect(newKeys(shown, after)).toEqual(['video:n2', 'video:n1']);
    // Nothing new: the same list.
    expect(keys(withFirstPage(shown, shown.slice(0, 2), false))).toEqual(keys(shown));
  });

  it('keeps the page’s order for cards it puts further down', () => {
    const first = [
      video('v3', 'a', '2026-10-21T10:00:00Z'),
      video('late', 'c', '2026-10-21T09:30:00Z'),
      video('v2', 'a', '2026-10-21T09:00:00Z'),
      video('tail', 'c', '2026-10-21T08:30:00Z'),
    ];
    expect(keys(withFirstPage(shown, first, false))).toEqual([
      'video:v3',
      'video:late',
      'video:v2',
      'video:tail',
      'video:v1',
    ]);
    // A page with none of the list: all of it first.
    const apart = [video('x1', 'c', '2026-10-22T10:00:00Z')];
    expect(keys(withFirstPage(shown, apart, false))).toEqual(['video:x1', ...keys(shown)]);
  });

  it('takes new live streams only when asked', () => {
    const first: FeedItem[] = [
      { type: 'stream', stream: stream('fresh', '2026-10-21T11:00:00Z') },
      video('v3', 'a', '2026-10-21T10:00:00Z'),
    ];
    expect(keys(withFirstPage(shown, first, false))).toEqual(keys(shown));
    expect(keys(withFirstPage(shown, first, true))).toEqual(['stream:fresh', ...keys(shown)]);
  });
});

describe('freshList', () => {
  it('brings the first page in and the streams as they are live now', () => {
    const items: FeedItem[] = [
      { type: 'stream', stream: stream('ended', '2026-10-21T11:00:00Z') },
      video('v1', 'a', '2026-10-21T09:00:00Z'),
    ];
    const first = [
      video('n1', 'b', '2026-10-21T12:00:00Z'),
      video('v1', 'a', '2026-10-21T09:00:00Z'),
    ];
    expect(keys(freshList(items, first, [], false, false))).toEqual(['video:n1', 'video:v1']);
    // Without an answer about live streams they stay; the new card goes before
    // the first card of the page that the list has.
    expect(keys(freshList(items, first, null, false, false))).toEqual([
      'stream:ended',
      'video:n1',
      'video:v1',
    ]);
    // Under the Streams chip new live ones come in by their time.
    const live = [stream('fresh', '2026-10-21T10:00:00Z')];
    expect(keys(freshList(items, first, live, true, false))).toEqual([
      'video:n1',
      'stream:fresh',
      'video:v1',
    ]);
  });
});
