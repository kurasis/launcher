import { describe, expect, it } from 'vitest';
import type { FeedItem, FeedStream } from '../../lib/contracts';
import { itemKey, mergeByTime, withLiveStreams } from './list';

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
    },
  };
}

function stream(login: string, started: string, viewers = 10): FeedStream {
  return { login, name: login, title: `${login} live`, viewers, started, image: null };
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
  it('drops the streams that ended, updates the others and adds new ones by time', () => {
    const items: FeedItem[] = [
      { type: 'stream', stream: stream('ended', '2026-10-21T11:00:00Z') },
      { type: 'stream', stream: stream('going', '2026-10-21T09:30:00Z', 5) },
      video('v1', 'a', '2026-10-21T09:00:00Z'),
      video('v2', 'a', '2026-10-21T07:00:00Z'),
    ];
    const after = withLiveStreams(items, [
      stream('going', '2026-10-21T09:30:00Z', 500),
      stream('fresh', '2026-10-21T08:00:00Z'),
    ]);
    expect(keys(after)).toEqual(['stream:going', 'video:v1', 'stream:fresh', 'video:v2']);
    const going = after[0];
    expect(going?.type === 'stream' && going.stream.viewers).toBe(500);
    // With nothing live, the other items stay as they were.
    expect(keys(withLiveStreams(items, []))).toEqual(['video:v1', 'video:v2']);
  });
});
