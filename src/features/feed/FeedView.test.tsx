import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type {
  AppSnapshot,
  ChannelMark,
  Feed,
  FeedItem,
  FeedMarks,
  FeedPage,
  FeedSettings,
  FeedView as FeedAnswer,
  Note,
  NoteView,
} from '../../lib/contracts';
import { loadLanguage, setLanguage, strings } from '../../lib/strings';
import { instanceFixture, snapshotFixture } from '../../test/fixtures';
import { FeedView } from './FeedView';
import { pictureUrl } from './pictureUrl';

const SHA = '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef';

const newsCard: Feed['news'][number] = {
  id: 'n-a-new-mob',
  title: 'A new mob',
  date: '2026-10-10T08:00:00Z',
  summary: 'Meet it.',
  translated: true,
  version: null,
  kind: null,
  image: null,
  url: 'https://www.minecraft.net/en-us/article/a-new-mob',
};

function feedFixture(overrides: Partial<Feed> = {}): Feed {
  return {
    format: 1,
    generated: '2026-10-19T12:00:00Z',
    lang: 'en',
    versions: [
      {
        id: 'j-26-4-snapshot-4',
        title: 'Minecraft 26.4 Snapshot 4',
        date: '2026-10-14T15:00:00Z',
        summary: 'New mobs and fixes.',
        translated: false,
        version: '26.4-snapshot-4',
        kind: 'snapshot',
        image: `img/${SHA}.jpg`,
        url: null,
      },
      {
        id: 'j-26-3',
        title: 'Minecraft 26.3',
        date: '2026-09-01T10:00:00Z',
        summary: 'The release.',
        translated: false,
        version: '26.3',
        kind: 'release',
        image: null,
        url: null,
      },
    ],
    news: [newsCard],
    videos: [
      {
        id: 'dQw4w9WgXcQ',
        channelId: 'UCabcdefghijklmnopqrstuv',
        channel: 'Builder',
        title: 'A long video',
        date: '2026-10-18T09:00:00Z',
        short: false,
        image: 'yt/dQw4w9WgXcQ.jpg',
      },
      {
        id: 'shortshort1',
        channelId: 'UCabcdefghijklmnopqrstuv',
        channel: 'Builder',
        title: 'A short one',
        date: '2026-10-18T10:00:00Z',
        short: true,
        image: 'yt/shortshort1.jpg',
      },
    ],
    streams: [
      {
        login: 'some_streamer',
        name: 'Some Streamer',
        title: 'Building a castle',
        viewers: 1234,
        started: '2026-10-19T10:00:00Z',
        image: 'tw/some_streamer.jpg',
      },
    ],
    cycle: {
      target: '26.4',
      snapshots: 4,
      pre: 0,
      rc: 0,
      released: false,
      stage: 'snapshot',
      latest: '26.4-snapshot-4',
      date: '2026-10-14T15:00:00Z',
    },
    latest: {
      release: { id: '26.3', kind: 'release', date: '2026-09-01T10:00:00Z' },
      snapshot: { id: '26.4-snapshot-4', kind: 'snapshot', date: '2026-10-14T15:00:00Z' },
    },
    events: [
      {
        id: 'build-contest',
        title: 'Build contest',
        starts: '2026-10-25T10:00:00Z',
        ends: '2026-10-26T10:00:00Z',
        url: 'https://www.youtube.com/watch?v=dQw4w9WgXcQ',
      },
    ],
    announcements: [{ id: 'welcome', title: 'Welcome', text: 'Hello there.', url: null }],
    tip: 'Press F3 for coordinates.',
    fetched: {},
    ...overrides,
  };
}

function noteFixture(original: boolean): Note {
  return {
    ...newsCard,
    translated: !original,
    title: original ? 'A new mob (original)' : 'A new mob',
    lang: original ? 'en' : 'ru',
    blocks: [
      { t: 'h', level: 1, c: [{ text: 'The mob' }] },
      {
        t: 'p',
        c: [
          { text: '<img src=x onerror=alert(1)>' },
          { text: ' bold', b: true },
          { text: ' a link', href: 'https://www.minecraft.net/en-us' },
        ],
      },
      { t: 'ul', items: [[{ t: 'p', c: [{ text: 'first point' }] }]] },
      { t: 'img', src: `img/${SHA}.jpg`, alt: 'The mob in a forest' },
      { t: 'img', src: 'https://evil.example/x.jpg', alt: 'remote' },
    ],
  };
}

/** The newest version's whole note. */
function versionNote(): NoteView {
  const [card] = feedFixture().versions;
  if (!card) throw new Error('the fixture changed');
  return { note: { ...noteFixture(false), ...card, lang: 'en', blocks: [] }, stale: false };
}

function snapshotWith(feed: Partial<FeedSettings> = {}, more: Partial<AppSnapshot> = {}) {
  const base = snapshotFixture();
  return snapshotFixture({
    ...more,
    settings: { ...base.settings, startView: 'feed', feed: { ...base.settings.feed, ...feed } },
  });
}

type Handler = (args: Record<string, unknown>) => unknown;

/** What the service would page through for the default settings: no Shorts. */
function serviceItems(feed: Feed = feedFixture()): FeedItem[] {
  const [snapshot, release] = feed.versions;
  const [stream] = feed.streams;
  const [video] = feed.videos;
  if (!snapshot || !release || !stream || !video) throw new Error('the fixture changed');
  return [
    { type: 'stream', stream },
    { type: 'video', video },
    { type: 'note', note: snapshot },
    { type: 'note', note: newsCard },
    { type: 'note', note: release },
  ];
}

const PAGE = 2;

/** Pages of `items`, `PAGE` at a time, as the core answers `feed_page`. */
function pager(items: FeedItem[] = serviceItems()): Handler {
  return (args) => {
    const part = (item: FeedItem) =>
      item.type === 'note'
        ? item.note.id.startsWith('j-')
          ? 'versions'
          : 'news'
        : `${item.type}s`;
    const shown = items.filter((item) => args.filter === null || part(item) === args.filter);
    const start = typeof args.before === 'string' ? Number(args.before.slice(1)) : 0;
    const end = start + PAGE;
    const page: FeedPage = {
      items: shown.slice(start, end),
      next: end < shown.length ? `c${String(end)}` : null,
    };
    return page;
  };
}

/** The titles of the list's items, in order. */
function titles(name: string) {
  return within(screen.getByRole('list', { name }))
    .getAllByRole('heading', { level: 3 })
    .map((h) => h.textContent);
}

function backend(overrides: Record<string, Handler> = {}, answer?: FeedAnswer) {
  const calls: { cmd: string; args: Record<string, unknown> }[] = [];
  let marks: FeedMarks = { hidden: [], favorite: [] };
  mockIPC((cmd, payload) => {
    const args = (payload ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args });
    const override = overrides[cmd];
    if (override !== undefined) return override(args);
    switch (cmd) {
      case 'feed_get':
        return answer ?? { feed: feedFixture(), stale: false, problem: null };
      case 'feed_own_videos':
      case 'feed_own_streams':
        return [];
      case 'feed_marks':
        return marks;
      case 'feed_mark': {
        const channel = { kind: args.kind, id: args.id, name: args.name } as ChannelMark;
        const other = (list: ChannelMark[]) =>
          list.filter((m) => m.kind !== channel.kind || m.id !== channel.id);
        marks = {
          hidden: [...other(marks.hidden), ...(args.list === 'hidden' ? [channel] : [])],
          favorite: [...other(marks.favorite), ...(args.list === 'favorite' ? [channel] : [])],
        };
        return marks;
      }
      case 'feed_favorites':
        return { videos: [], streams: [] };
      case 'feed_watched':
        return [];
      case 'feed_server_ads':
        return { ads: [], stale: false };
      case 'feed_page':
        return pager()(args);
      case 'feed_note': {
        const view: NoteView = { note: noteFixture(args.original === true), stale: false };
        return view;
      }
      default:
        return null;
    }
  });
  return calls;
}

function show(snapshot: AppSnapshot = snapshotWith()) {
  const onOpenInstallations = vi.fn();
  const onInstallVersion = vi.fn();
  render(
    <FeedView
      snapshot={snapshot}
      onOpenInstallations={onOpenInstallations}
      onInstallVersion={onInstallVersion}
    />,
  );
  return { onOpenInstallations, onInstallVersion };
}

describe('FeedView', () => {
  afterEach(() => {
    setLanguage('en');
  });

  it('shows the newest version, one mixed list and the side column', async () => {
    const calls = backend();
    const { onInstallVersion } = show();
    const hero = await screen.findByRole('region', { name: 'Minecraft 26.4 Snapshot 4' });
    expect(within(hero).getByText('Snapshot')).toBeVisible();
    fireEvent.click(within(hero).getByRole('button', { name: 'Install 26.4-snapshot-4' }));
    expect(onInstallVersion).toHaveBeenCalledWith('26.4-snapshot-4');

    // Streams, videos and articles in one list, newest first, a page at a time.
    await screen.findByRole('list', { name: 'All' });
    expect(titles('All')).toEqual(['Building a castle', 'A long video']);
    const stream = screen.getByRole('button', {
      name: 'Watch Some Streamer live: Building a castle (1,234 viewers)',
    });
    expect(stream).toBeVisible();
    expect(screen.getByText('Live')).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Show more' }));
    // The newest version is above the list, not in it again.
    await waitFor(() => {
      expect(titles('All')).toEqual(['Building a castle', 'A long video', 'A new mob']);
    });
    expect(screen.queryByText('Machine-translated')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Show more' }));
    expect(await screen.findByText('That’s everything for now.')).toBeVisible();
    expect(titles('All')).toEqual([
      'Building a castle',
      'A long video',
      'A new mob',
      'Minecraft 26.3',
    ]);
    expect(screen.queryByRole('button', { name: 'Show more' })).toBeNull();
    expect(calls.filter((c) => c.cmd === 'feed_page').map((c) => c.args)).toEqual([
      { filter: null, shortsOnly: false, before: null },
      { filter: null, shortsOnly: false, before: 'c2' },
      { filter: null, shortsOnly: false, before: 'c4' },
    ]);

    const side = screen.getByRole('complementary', { name: 'Updates, events and tips' });
    expect(within(side).getByText('Next: 26.4')).toBeVisible();
    expect(within(side).getByText('4 snapshots')).toBeVisible();
    expect(within(side).getByText('Build contest')).toBeVisible();
    expect(within(side).getByText('Hello there.')).toBeVisible();
    expect(within(side).getByText('Press F3 for coordinates.')).toBeVisible();
  });

  it('puts a server ad second in All, and not in the other parts', async () => {
    const ad = {
      id: '7',
      name: 'Northern Land',
      address: 'play.example.net',
      text: 'Survival with clans.',
      version: '26.3',
      versions: ['26.3'],
      icon: null,
      up: null,
      online: null,
      max: null,
      endsAt: Math.floor(Date.now() / 1000) + 3600,
      weight: 10,
      erid: null,
      banner: null,
      licensed: null,
    };
    const calls = backend({ feed_server_ads: () => ({ ads: [ad], stale: false }) });
    show();
    const list = await screen.findByRole('list', { name: 'All' });
    const tile = await within(list).findByRole('article', { name: 'Ad: Northern Land' });
    expect(tile.closest('li')).toBe(list.children[1]);
    expect(calls.find((c) => c.cmd === 'feed_server_ads')?.args).toEqual({
      place: 'feed',
      refresh: false,
    });
    fireEvent.click(screen.getByRole('button', { name: 'Videos' }));
    await screen.findByRole('list', { name: 'Videos' });
    expect(screen.queryByRole('article', { name: 'Ad: Northern Land' })).toBeNull();
  });

  it('asks for the next page by itself as the end of the list comes near', async () => {
    const watched: ((entries: { isIntersecting: boolean }[]) => void)[] = [];
    class Watcher {
      constructor(callback: (entries: { isIntersecting: boolean }[]) => void) {
        watched.push(callback);
      }
      observe() {}
      disconnect() {}
    }
    vi.stubGlobal('IntersectionObserver', Watcher);
    try {
      const calls = backend();
      show();
      await screen.findByRole('list', { name: 'All' });
      // No button: the list loads more on its own.
      expect(screen.queryByRole('button', { name: 'Show more' })).toBeNull();
      // The list can start again while the feed settles, leaving an earlier
      // watcher behind, so the newest one is told until the page comes.
      await waitFor(() => {
        watched.at(-1)?.([{ isIntersecting: true }]);
        expect(titles('All')).toContain('A new mob');
      });
      expect(calls.filter((c) => c.cmd === 'feed_page').at(-1)?.args).toEqual({
        filter: null,
        shortsOnly: false,
        before: 'c2',
      });
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it('says when more cannot be loaded, and tries again', async () => {
    let fail = true;
    const pages = pager();
    backend({
      feed_page: (args) => {
        if (args.before !== null && fail) {
          // eslint-disable-next-line @typescript-eslint/only-throw-error
          throw 'offline';
        }
        return pages(args);
      },
    });
    show();
    fireEvent.click(await screen.findByRole('button', { name: 'Show more' }));
    expect(await screen.findByText('Couldn’t load more of the feed.')).toBeVisible();
    fail = false;
    fireEvent.click(screen.getByRole('button', { name: 'Try again' }));
    await waitFor(() => {
      expect(titles('All')).toContain('A new mob');
    });
  });

  it('loads pictures only from the feed protocol', async () => {
    backend();
    show();
    await screen.findByRole('list', { name: 'All' });
    const sources = Array.from(document.querySelectorAll('img'), (img) => img.getAttribute('src'));
    expect(sources.length).toBeGreaterThan(0);
    for (const src of sources) expect(src).toMatch(/^feed:\/\/localhost\/(img|yt|tw)\//);
    expect(pictureUrl('yt/../../settings.json')).toBeNull();
    expect(pictureUrl('https://evil.example/x.jpg')).toBeNull();
    expect(pictureUrl(`img/${SHA}.jpg`)).toBe(`feed://localhost/img/${SHA}.jpg`);
  });

  it('filters by kind with the chips', async () => {
    const calls = backend();
    show();
    const chips = await screen.findByRole('group', { name: 'Show in the feed' });
    expect(
      within(chips)
        .getAllByRole('button')
        .map((b) => b.textContent),
    ).toEqual(['All', 'Versions', 'News', 'Videos', 'Streams']);
    fireEvent.click(within(chips).getByRole('button', { name: 'News' }));
    expect(within(chips).getByRole('button', { name: 'News' })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
    await screen.findByRole('list', { name: 'News' });
    expect(titles('News')).toEqual(['A new mob']);
    fireEvent.click(within(chips).getByRole('button', { name: 'Videos' }));
    await screen.findByRole('list', { name: 'Videos' });
    expect(titles('Videos')).toEqual(['A long video']);
    expect(screen.queryByRole('button', { name: /Some Streamer live/ })).toBeNull();
    expect(
      calls
        .filter((c) => c.cmd === 'feed_page')
        .map((c) => c.args.filter)
        .slice(-2),
    ).toEqual(['news', 'videos']);
  });

  it('shows the feed the core has when pages cannot be fetched, as the settings say', async () => {
    backend({
      feed_page: () => {
        // eslint-disable-next-line @typescript-eslint/only-throw-error
        throw 'not_found';
      },
    });
    show(snapshotWith({ snapshots: false, shorts: true, hidden: ['streams', 'news'] }));
    // Without snapshots, the newest version is the release, above the list.
    expect(await screen.findByRole('button', { name: 'Install 26.3' })).toBeVisible();
    await screen.findByRole('list', { name: 'All' });
    expect(titles('All')).toEqual(['A short one', 'A long video']);
    const list = screen.getByRole('list', { name: 'All' });
    expect(within(list).getByText('Shorts')).toBeVisible();
    expect(screen.queryByRole('button', { name: 'Show more' })).toBeNull();
    expect(screen.queryByRole('region', { name: 'Update cycle' })).toBeNull();
    const chips = screen.getByRole('group', { name: 'Show in the feed' });
    // With Shorts shown, they have a chip of their own.
    expect(
      within(chips)
        .getAllByRole('button')
        .map((b) => b.textContent),
    ).toEqual(['All', 'Versions', 'Videos', 'Shorts']);
    fireEvent.click(within(chips).getByRole('button', { name: 'Shorts' }));
    await screen.findByRole('list', { name: 'Shorts' });
    expect(titles('Shorts')).toEqual(['A short one']);
  });

  it('asks the core for Shorts alone under their chip', async () => {
    const calls = backend();
    show(snapshotWith({ shorts: true }));
    const chips = await screen.findByRole('group', { name: 'Show in the feed' });
    fireEvent.click(within(chips).getByRole('button', { name: 'Shorts' }));
    await waitFor(() => {
      expect(calls.filter((c) => c.cmd === 'feed_page').at(-1)?.args).toEqual({
        filter: 'videos',
        shortsOnly: true,
        before: null,
      });
    });
  });

  it('opens a note and renders its blocks as text, never as HTML', async () => {
    const calls = backend();
    show();
    fireEvent.click(await screen.findByRole('button', { name: 'Show more' }));
    fireEvent.click(await screen.findByRole('button', { name: 'A new mob' }));
    const dialog = await screen.findByRole('dialog', { name: 'A new mob' });
    expect(await within(dialog).findByText('<img src=x onerror=alert(1)>')).toBeVisible();
    expect(dialog.querySelector('img[src="x"]')).toBeNull();
    expect(within(dialog).getByRole('heading', { level: 3, name: 'The mob' })).toBeVisible();
    expect(within(dialog).getByText('bold').tagName).toBe('STRONG');
    expect(within(dialog).getByText('first point')).toBeVisible();
    expect(within(dialog).getByRole('img', { name: 'The mob in a forest' })).toHaveAttribute(
      'src',
      `feed://localhost/img/${SHA}.jpg`,
    );
    expect(within(dialog).queryByRole('img', { name: 'remote' })).toBeNull();
    expect(dialog.querySelector('a, script, iframe')).toBeNull();

    // Links go to the core, which opens them in the browser.
    fireEvent.click(within(dialog).getByRole('button', { name: 'a link' }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'feed_open_link')?.args).toEqual({
        url: 'https://www.minecraft.net/en-us',
      });
    });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Open on minecraft.net' }));
    await waitFor(() => {
      expect(calls.filter((c) => c.cmd === 'feed_open_link').at(-1)?.args).toEqual({
        url: 'https://www.minecraft.net/en-us/article/a-new-mob',
      });
    });

    // A machine translation shows its original on request, without a label.
    expect(within(dialog).queryByText('Machine-translated')).toBeNull();
    const original = within(dialog).getByRole('button', { name: 'Original' });
    expect(original).toHaveAttribute('aria-pressed', 'false');
    fireEvent.click(original);
    expect(original).toHaveAttribute('aria-pressed', 'true');
    expect(await screen.findByRole('dialog', { name: 'A new mob (original)' })).toBeInTheDocument();
    expect(calls.filter((c) => c.cmd === 'feed_note').map((c) => c.args)).toEqual([
      { id: 'n-a-new-mob', original: false },
      { id: 'n-a-new-mob', original: true },
    ]);

    fireEvent.keyDown(window, { key: 'Escape' });
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('says when a note cannot be loaded', async () => {
    backend({
      feed_note: () => {
        // The core rejects with the problem's name, a plain string.
        // eslint-disable-next-line @typescript-eslint/only-throw-error
        throw 'offline';
      },
    });
    show();
    fireEvent.click(await screen.findByRole('button', { name: 'Read the patch notes' }));
    const dialog = await screen.findByRole('dialog', { name: 'Minecraft 26.4 Snapshot 4' });
    expect(await within(dialog).findByRole('alert')).toHaveTextContent(
      'The feed cannot be reached. Check the internet connection.',
    );
    fireEvent.click(within(dialog).getByRole('button', { name: 'Close' }));
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('plays videos and streams through the core', async () => {
    const calls = backend();
    show();
    fireEvent.click(await screen.findByRole('button', { name: 'Watch “A long video” by Builder' }));
    fireEvent.click(
      screen.getByRole('button', {
        name: 'Watch Some Streamer live: Building a castle (1,234 viewers)',
      }),
    );
    await waitFor(() => {
      expect(calls.filter((c) => c.cmd === 'feed_play').map((c) => c.args)).toEqual([
        { kind: 'yt', id: 'dQw4w9WgXcQ', title: 'Video' },
        { kind: 'twitch', id: 'some_streamer', title: 'Video' },
      ]);
    });
  });

  it('shows the videos played from the feed as watched', async () => {
    const calls = backend();
    show();
    const play = await screen.findByRole('button', { name: /^Watch “A long video”/ });
    const tile = play.closest('article');
    if (tile === null) throw new Error('no tile');
    expect(within(tile).queryByText('Watched')).toBeNull();
    expect(calls.filter((c) => c.cmd === 'feed_watched')).toHaveLength(1);
    fireEvent.click(play);
    expect(await within(tile).findByText('Watched')).toBeVisible();
    cleanup();
    // The core remembers them, so they show so when the feed opens again.
    backend({ feed_watched: () => ['dQw4w9WgXcQ'] });
    show();
    const again = (await screen.findByRole('button', { name: /^Watch “A long video”/ })).closest(
      'article',
    );
    if (again === null) throw new Error('no tile');
    expect(await within(again).findByText('Watched')).toBeVisible();
  });

  it('asks for the live streams again every few minutes: ended ones leave, new ones come', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const [stream] = feedFixture().streams;
      if (!stream) throw new Error('the fixture changed');
      const later = {
        login: 'ph1lza',
        name: 'Ph1LzA',
        title: 'Hardcore day 1',
        viewers: 50,
        started: '2026-10-19T11:30:00Z',
        image: null,
      };
      let live = [stream];
      const calls = backend({
        feed_page: (args) =>
          args.filter === 'streams'
            ? {
                items: live.map((item): FeedItem => ({ type: 'stream', stream: item })),
                next: null,
              }
            : pager()(args),
      });
      show();
      await screen.findByRole('list', { name: 'All' });
      expect(titles('All')).toEqual(['Building a castle', 'A long video']);
      live = [later];
      await vi.advanceTimersByTimeAsync(3 * 60 * 1000);
      await waitFor(() => {
        expect(titles('All')).toEqual(['Hardcore day 1', 'A long video']);
      });
      expect(
        calls.filter((c) => c.cmd === 'feed_page' && c.args.filter === 'streams'),
      ).toHaveLength(1);
      expect(calls.filter((c) => c.cmd === 'feed_own_streams').length).toBeGreaterThanOrEqual(2);
    } finally {
      vi.useRealTimers();
    }
  });

  it('hides a channel from a tile’s menu, and Undo brings it back', async () => {
    const calls = backend();
    show();
    await screen.findByRole('list', { name: 'All' });
    fireEvent.click(screen.getByRole('button', { name: 'Show more' }));
    fireEvent.click(await screen.findByRole('button', { name: 'More: A long video' }));
    const menu = screen.getByRole('menu');
    expect(
      within(menu)
        .getAllByRole('menuitem')
        .map((item) => item.textContent),
    ).toEqual(['Copy link', 'Show Builder first', 'Don’t show Builder']);
    fireEvent.click(within(menu).getByRole('menuitem', { name: 'Don’t show Builder' }));
    expect(await screen.findByText('Builder is hidden from the feed.')).toBeVisible();
    expect(screen.queryByRole('button', { name: /^Watch “A long video”/ })).toBeNull();
    expect(calls.find((c) => c.cmd === 'feed_mark')?.args).toEqual({
      kind: 'youtube',
      id: 'UCabcdefghijklmnopqrstuv',
      name: 'Builder',
      list: 'hidden',
    });
    fireEvent.click(screen.getByRole('button', { name: 'Undo' }));
    expect(await screen.findByRole('button', { name: /^Watch “A long video”/ })).toBeVisible();
    expect(calls.filter((c) => c.cmd === 'feed_mark').at(-1)?.args.list).toBeNull();
  });

  it('shows the favourite channels’ videos and streams first', async () => {
    const old = {
      id: 'oldvideo001',
      channelId: 'UCzzzzzzzzzzzzzzzzzzzzzz',
      channel: 'Loved',
      title: 'An old favourite',
      date: '2026-01-01T00:00:00Z',
      short: false,
      image: null,
    };
    backend({
      feed_marks: () => ({
        hidden: [],
        favorite: [{ kind: 'youtube', id: 'UCzzzzzzzzzzzzzzzzzzzzzz', name: 'Loved' }],
      }),
      feed_favorites: () => ({ videos: [old], streams: [] }),
    });
    show();
    await screen.findByRole('list', { name: 'All' });
    await waitFor(() => {
      expect(titles('All')[0]).toBe('An old favourite');
    });
    const tile = screen
      .getByRole('button', { name: /^Watch “An old favourite”/ })
      .closest('article');
    expect(tile).not.toBeNull();
    if (tile !== null) expect(within(tile).getByText('Favourite')).toBeVisible();
    // Taken off, it goes back to its own place at once.
    fireEvent.click(screen.getByRole('button', { name: 'More: An old favourite' }));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Stop showing Loved first' }));
    await waitFor(() => {
      expect(titles('All')[0]).toBe('Building a castle');
    });
  });

  it('copies a stream’s link', async () => {
    const writeText = vi.fn(() => Promise.resolve());
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    backend();
    show();
    fireEvent.click(await screen.findByRole('button', { name: 'More: Building a castle' }));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Copy link' }));
    expect(writeText).toHaveBeenCalledWith('https://www.twitch.tv/some_streamer');
    expect(await screen.findByText('Link copied.')).toBeVisible();
  });

  it('adds a Twitch channel from the + by the chips', async () => {
    const calls = backend({
      feed_streamers: () => [],
      feed_add_streamer: () => [{ login: 'ph1lza', name: 'Ph1LzA' }],
    });
    show();
    fireEvent.click(await screen.findByRole('button', { name: 'Add a channel' }));
    const dialog = screen.getByRole('dialog', { name: 'Add a channel' });
    expect(within(dialog).getByRole('radio', { name: 'YouTube' })).toBeChecked();
    fireEvent.change(within(dialog).getByLabelText('Channel link, @handle or channel ID'), {
      target: { value: 'https://www.twitch.tv/ph1lza' },
    });
    expect(within(dialog).getByRole('radio', { name: 'Twitch' })).toBeChecked();
    const before = calls.filter((c) => c.cmd === 'feed_own_streams').length;
    fireEvent.click(within(dialog).getByRole('button', { name: 'Add' }));
    expect(await screen.findByText('Ph1LzA is added to the feed.')).toBeVisible();
    expect(calls.find((c) => c.cmd === 'feed_add_streamer')?.args).toEqual({
      input: 'https://www.twitch.tv/ph1lza',
    });
    expect(screen.queryByRole('dialog', { name: 'Add a channel' })).toBeNull();
    await waitFor(() => {
      expect(calls.filter((c) => c.cmd === 'feed_own_streams').length).toBeGreaterThan(before);
    });
  });

  it('says why a channel from the + was not added', async () => {
    backend({
      feed_channels: () => [],
      feed_add_channel: () => {
        // eslint-disable-next-line @typescript-eslint/only-throw-error
        throw 'not_found';
      },
    });
    show();
    fireEvent.click(await screen.findByRole('button', { name: 'Add a channel' }));
    const dialog = screen.getByRole('dialog', { name: 'Add a channel' });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Add' }));
    expect(await within(dialog).findByRole('alert')).toBeVisible();
    fireEvent.change(within(dialog).getByRole('textbox'), { target: { value: '@nobody' } });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Add' }));
    await waitFor(() => {
      expect(within(dialog).getByRole('alert')).toHaveTextContent(strings.feed.problems.not_found);
    });
    fireEvent.keyDown(within(dialog).getByRole('textbox'), { key: 'Escape' });
    expect(screen.queryByRole('dialog', { name: 'Add a channel' })).toBeNull();
  });

  it('says when a video cannot be opened', async () => {
    backend({
      feed_play: () => {
        // eslint-disable-next-line @typescript-eslint/only-throw-error
        throw 'server';
      },
    });
    show();
    fireEvent.click(await screen.findByRole('button', { name: /^Watch “A long video”/ }));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'The feed did not answer as expected.',
    );
  });

  it('shows the copy kept on disk when offline, and errors in words', async () => {
    backend({}, { feed: feedFixture(), stale: true, problem: 'offline' });
    show();
    expect(await screen.findByText(/this is the copy from/)).toBeVisible();
    expect(await screen.findByRole('list', { name: 'All' })).toBeVisible();
  });

  it('says why there is no feed, and asks again on Refresh', async () => {
    let fail = true;
    const calls = backend({
      feed_get: () => {
        if (fail) {
          // eslint-disable-next-line @typescript-eslint/only-throw-error
          throw 'offline';
        }
        return { feed: feedFixture(), stale: false, problem: null };
      },
    });
    show();
    expect(await screen.findByRole('alert')).toHaveTextContent('The feed cannot be reached.');
    fail = false;
    fireEvent.click(screen.getByRole('button', { name: 'Refresh' }));
    expect(await screen.findByRole('list', { name: 'All' })).toBeVisible();
    expect(calls.filter((c) => c.cmd === 'feed_get').at(-1)?.args).toEqual({ refresh: true });
  });

  it('shows empty parts in words', async () => {
    backend(
      { feed_page: () => ({ items: [], next: null }) },
      {
        feed: feedFixture({ versions: [], news: [], videos: [], streams: [], cycle: null }),
        stale: false,
        problem: null,
      },
    );
    show(snapshotWith({}, { instances: [] }));
    expect(await screen.findByText('Nothing here yet.')).toBeVisible();
    // The newest release still has its card.
    expect(screen.getByRole('region', { name: 'Minecraft 26.3' })).toBeVisible();
    const chips = screen.getByRole('group', { name: 'Show in the feed' });
    fireEvent.click(within(chips).getByRole('button', { name: 'News' }));
    expect(await screen.findByText('No patch notes or news here yet.')).toBeVisible();
    fireEvent.click(within(chips).getByRole('button', { name: 'Videos' }));
    expect(await screen.findByText('No videos yet.')).toBeVisible();
    fireEvent.click(within(chips).getByRole('button', { name: 'Streams' }));
    expect(await screen.findByText('No one is streaming Minecraft right now.')).toBeVisible();
  });

  it('continues the installation played last', async () => {
    const calls = backend();
    const played = instanceFixture({
      id: '11111111-2222-4333-8444-555555555555',
      displayName: 'Creative',
      health: 'ready',
      lastPlayedAt: '2026-10-18T12:00:00Z',
    });
    const older = instanceFixture({ lastPlayedAt: '2026-10-01T12:00:00Z', health: 'ready' });
    const { onOpenInstallations } = show(snapshotWith({}, { instances: [older, played] }));
    const card = await screen.findByRole('region', { name: 'Continue playing' });
    expect(within(card).getByText(/Creative/)).toBeVisible();
    fireEvent.click(within(card).getByRole('button', { name: 'Play' }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'launch_game')?.args).toMatchObject({
        request: { payload: { instanceId: played.id } },
      });
    });
    fireEvent.click(within(card).getByRole('button', { name: 'Installations' }));
    expect(onOpenInstallations).toHaveBeenCalledWith(played.id);
  });

  it('shows videos of the player’s own channels', async () => {
    backend({
      feed_own_videos: () => [
        {
          id: 'ownvideo123',
          channelId: 'UCzzzzzzzzzzzzzzzzzzzzzz',
          channel: 'Mine',
          title: 'My build',
          date: '2026-10-17T09:00:00Z',
          short: false,
          image: null,
        },
      ],
    });
    show();
    await screen.findByRole('list', { name: 'All' });
    // It waits until the pages reach its time, so the order holds.
    expect(screen.queryByRole('button', { name: 'Watch “My build” by Mine' })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Show more' }));
    expect(await screen.findByRole('button', { name: 'Watch “My build” by Mine' })).toBeVisible();
    expect(titles('All')).toEqual(['Building a castle', 'A long video', 'My build', 'A new mob']);
  });

  it('streams of the player’s own Twitch channels go in the list', async () => {
    backend({
      feed_own_streams: () => [
        {
          login: 'ph1lza',
          name: 'Ph1LzA',
          title: 'Hardcore day 1',
          viewers: 50,
          started: '2026-10-19T11:00:00Z',
          image: null,
        },
      ],
    });
    show();
    await screen.findByRole('list', { name: 'All' });
    expect(
      await screen.findByRole('button', { name: 'Watch Ph1LzA live: Hardcore day 1 (50 viewers)' }),
    ).toBeVisible();
    expect(titles('All')[0]).toBe('Hardcore day 1');
  });

  it('a version’s note plays that version where it is installed, and installs it elsewhere', async () => {
    const calls = backend({ feed_note: versionNote });
    const ready = instanceFixture({
      id: '11111111-2222-4333-8444-555555555555',
      versionId: '26.4-snapshot-4',
      health: 'ready',
    });
    show(snapshotWith({}, { instances: [ready] }));
    const hero = await screen.findByRole('region', { name: 'Minecraft 26.4 Snapshot 4' });
    expect(within(hero).getByRole('button', { name: 'Play 26.4-snapshot-4' })).toBeVisible();
    fireEvent.click(within(hero).getByRole('button', { name: 'Read the patch notes' }));
    const dialog = await screen.findByRole('dialog', { name: 'Minecraft 26.4 Snapshot 4' });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Play 26.4-snapshot-4' }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'launch_game')?.args).toMatchObject({
        request: { payload: { instanceId: ready.id } },
      });
    });
  });

  it('a version’s note installs it, or opens the installation not ready yet', async () => {
    backend({ feed_note: versionNote });
    const { onInstallVersion } = show();
    const hero = await screen.findByRole('region', { name: 'Minecraft 26.4 Snapshot 4' });
    fireEvent.click(within(hero).getByRole('button', { name: 'Read the patch notes' }));
    let dialog = await screen.findByRole('dialog', { name: 'Minecraft 26.4 Snapshot 4' });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Install 26.4-snapshot-4' }));
    expect(onInstallVersion).toHaveBeenCalledWith('26.4-snapshot-4');
    cleanup();

    const half = instanceFixture({
      id: '11111111-2222-4333-8444-666666666666',
      versionId: '26.4-snapshot-4',
      health: 'not_installed',
    });
    const { onOpenInstallations } = show(snapshotWith({}, { instances: [half] }));
    fireEvent.click(
      await screen.findByRole('button', { name: 'Read the patch notes' }, { timeout: 2000 }),
    );
    dialog = await screen.findByRole('dialog', { name: 'Minecraft 26.4 Snapshot 4' });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Install 26.4-snapshot-4' }));
    expect(onOpenInstallations).toHaveBeenCalledWith(half.id);
  });

  it('reads right to left in Arabic', async () => {
    await loadLanguage('ar');
    setLanguage('ar');
    backend();
    show();
    expect(document.documentElement.dir).toBe('rtl');
    expect(await screen.findByRole('heading', { level: 2, name: 'الموجز' })).toBeVisible();
    const chips = screen.getByRole('group', { name: 'ما يُعرض في الموجز' });
    expect(within(chips).getByRole('button', { name: 'الكل' })).toBeVisible();
    // Version ids stay left to right inside the Arabic text.
    expect(screen.getAllByText('26.4-snapshot-4')[0]).toHaveAttribute('dir', 'ltr');
  });
});
