import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type {
  AppSnapshot,
  Feed,
  FeedSettings,
  FeedView as FeedAnswer,
  Note,
  NoteView,
} from '../../lib/contracts';
import { loadLanguage, setLanguage } from '../../lib/strings';
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

function snapshotWith(feed: Partial<FeedSettings> = {}, more: Partial<AppSnapshot> = {}) {
  const base = snapshotFixture();
  return snapshotFixture({
    ...more,
    settings: { ...base.settings, startView: 'feed', feed: { ...base.settings.feed, ...feed } },
  });
}

type Handler = (args: Record<string, unknown>) => unknown;

function backend(overrides: Record<string, Handler> = {}, answer?: FeedAnswer) {
  const calls: { cmd: string; args: Record<string, unknown> }[] = [];
  mockIPC((cmd, payload) => {
    const args = (payload ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args });
    const override = overrides[cmd];
    if (override !== undefined) return override(args);
    switch (cmd) {
      case 'feed_get':
        return answer ?? { feed: feedFixture(), stale: false, problem: null };
      case 'feed_own_videos':
        return [];
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

function part(name: string) {
  return screen.getByRole('region', { name });
}

describe('FeedView', () => {
  afterEach(() => {
    setLanguage('en');
  });

  it('shows the newest version, articles, videos, streams and the side column', async () => {
    backend();
    const { onInstallVersion } = show();
    const hero = await screen.findByRole('region', { name: 'Minecraft 26.4 Snapshot 4' });
    expect(within(hero).getByText('Snapshot')).toBeVisible();
    fireEvent.click(within(hero).getByRole('button', { name: 'Install 26.4-snapshot-4' }));
    expect(onInstallVersion).toHaveBeenCalledWith('26.4-snapshot-4');

    const articles = part('Patch notes and news');
    expect(
      within(articles)
        .getAllByRole('heading', { level: 3 })
        .map((h) => h.textContent),
    ).toEqual(['Minecraft 26.4 Snapshot 4', 'A new mob', 'Minecraft 26.3']);
    expect(within(articles).getByText('Machine-translated')).toBeVisible();

    // Shorts are hidden by default.
    const videos = part('Videos');
    expect(within(videos).getByRole('button', { name: /A long video/ })).toBeVisible();
    expect(within(videos).queryByRole('button', { name: /A short one/ })).toBeNull();
    const streams = part('Live on Twitch');
    expect(within(streams).getByText(/1,234 viewers/)).toBeVisible();
    expect(within(streams).getByText('Live')).toBeVisible();

    const side = screen.getByRole('complementary', { name: 'Updates, events and tips' });
    expect(within(side).getByText('Next: 26.4')).toBeVisible();
    expect(within(side).getByText('4 snapshots')).toBeVisible();
    expect(within(side).getByText('Build contest')).toBeVisible();
    expect(within(side).getByText('Hello there.')).toBeVisible();
    expect(within(side).getByText('Press F3 for coordinates.')).toBeVisible();
  });

  it('loads pictures only from the feed protocol', async () => {
    backend();
    show();
    await screen.findByRole('region', { name: 'Videos' });
    const sources = Array.from(document.querySelectorAll('img'), (img) => img.getAttribute('src'));
    expect(sources.length).toBeGreaterThan(0);
    for (const src of sources) expect(src).toMatch(/^feed:\/\/localhost\/(img|yt|tw)\//);
    expect(pictureUrl('yt/../../settings.json')).toBeNull();
    expect(pictureUrl('https://evil.example/x.jpg')).toBeNull();
    expect(pictureUrl(`img/${SHA}.jpg`)).toBe(`feed://localhost/img/${SHA}.jpg`);
  });

  it('filters by kind with the chips', async () => {
    backend();
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
    expect(
      within(part('Patch notes and news'))
        .getAllByRole('heading', { level: 3 })
        .map((h) => h.textContent),
    ).toEqual(['A new mob']);
    expect(screen.queryByRole('region', { name: 'Videos' })).toBeNull();
    fireEvent.click(within(chips).getByRole('button', { name: 'Videos' }));
    expect(screen.queryByRole('region', { name: 'Patch notes and news' })).toBeNull();
    expect(part('Videos')).toBeVisible();
    expect(screen.queryByRole('region', { name: 'Live on Twitch' })).toBeNull();
  });

  it('follows the settings: snapshots, Shorts and hidden parts', async () => {
    backend();
    show(snapshotWith({ snapshots: false, shorts: true, hidden: ['streams', 'news'] }));
    const articles = await screen.findByRole('region', { name: 'Patch notes and news' });
    expect(
      within(articles)
        .getAllByRole('heading', { level: 3 })
        .map((h) => h.textContent),
    ).toEqual(['Minecraft 26.3']);
    // Without snapshots, the newest version is the release.
    expect(screen.getByRole('button', { name: 'Install 26.3' })).toBeVisible();
    expect(within(part('Videos')).getByRole('button', { name: /A short one/ })).toBeVisible();
    expect(within(part('Videos')).getByText('Shorts')).toBeVisible();
    expect(screen.queryByRole('region', { name: 'Live on Twitch' })).toBeNull();
    expect(screen.queryByRole('region', { name: 'Update cycle' })).toBeNull();
    const chips = screen.getByRole('group', { name: 'Show in the feed' });
    expect(
      within(chips)
        .getAllByRole('button')
        .map((b) => b.textContent),
    ).toEqual(['All', 'Versions', 'Videos']);
  });

  it('opens a note and renders its blocks as text, never as HTML', async () => {
    const calls = backend();
    show();
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

    // A machine translation says so and shows its original on request.
    expect(within(dialog).getByText('Machine-translated')).toBeVisible();
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

  it('says when a video cannot be opened', async () => {
    backend({
      feed_play: () => {
        // eslint-disable-next-line @typescript-eslint/only-throw-error
        throw 'server';
      },
    });
    show();
    fireEvent.click(await screen.findByRole('button', { name: /A long video/ }));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'The feed did not answer as expected.',
    );
  });

  it('shows the copy kept on disk when offline, and errors in words', async () => {
    backend({}, { feed: feedFixture(), stale: true, problem: 'offline' });
    show();
    expect(await screen.findByText(/this is the copy from/)).toBeVisible();
    expect(part('Patch notes and news')).toBeVisible();
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
    expect(await screen.findByRole('region', { name: 'Videos' })).toBeVisible();
    expect(calls.filter((c) => c.cmd === 'feed_get').at(-1)?.args).toEqual({ refresh: true });
  });

  it('shows empty parts in words', async () => {
    backend(
      {},
      {
        feed: feedFixture({ versions: [], news: [], videos: [], streams: [], cycle: null }),
        stale: false,
        problem: null,
      },
    );
    show(snapshotWith({}, { instances: [] }));
    expect(await screen.findByText('No patch notes or news here yet.')).toBeVisible();
    expect(screen.getByText('No videos yet.')).toBeVisible();
    expect(screen.getByText('No one is streaming Minecraft right now.')).toBeVisible();
    // The newest release still has its card.
    expect(screen.getByRole('region', { name: 'Minecraft 26.3' })).toBeVisible();
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
    const own = await screen.findByRole('region', { name: 'Your channels' });
    expect(within(own).getByRole('button', { name: 'Watch “My build” by Mine' })).toBeVisible();
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
