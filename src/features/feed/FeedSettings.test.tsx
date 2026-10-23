import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it } from 'vitest';
import type {
  FeedMarks,
  MutationRequest,
  OwnChannel,
  OwnStreamer,
  SettingsPatch,
} from '../../lib/contracts';
import { snapshotFixture } from '../../test/fixtures';
import { FeedSettings } from './FeedSettings';

const mine: OwnChannel = { channelId: 'UCabcdefghijklmnopqrstuv', title: 'Mine' };
const streamer: OwnStreamer = { login: 'ph1lza', name: 'Ph1LzA' };

function setup(
  add: (input: string) => OwnChannel[] = () => [mine],
  addStreamer: (input: string) => OwnStreamer[] = () => [streamer],
  startMarks: FeedMarks = { hidden: [], favorite: [] },
) {
  const calls: { cmd: string; args: Record<string, unknown> }[] = [];
  let channels: OwnChannel[] = [];
  let streamers: OwnStreamer[] = [];
  let marks: FeedMarks = startMarks;
  mockIPC((cmd, payload) => {
    const args = (payload ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args });
    switch (cmd) {
      case 'feed_channels':
        return channels;
      case 'feed_add_channel':
        channels = add(args.input as string);
        return channels;
      case 'feed_remove_channel':
        channels = channels.filter((c) => c.channelId !== args.channelId);
        return channels;
      case 'feed_streamers':
        return streamers;
      case 'feed_add_streamer':
        streamers = addStreamer(args.input as string);
        return streamers;
      case 'feed_remove_streamer':
        streamers = streamers.filter((s) => s.login !== args.login);
        return streamers;
      case 'feed_marks':
        return marks;
      case 'feed_mark':
        marks = {
          hidden: marks.hidden.filter((m) => m.id !== args.id),
          favorite: marks.favorite.filter((m) => m.id !== args.id),
        };
        return marks;
      case 'set_app_settings':
        return snapshotFixture().settings;
      default:
        return null;
    }
  });
  render(<FeedSettings settings={snapshotFixture().settings} />);
  return calls;
}

function youtube() {
  return within(screen.getByRole('group', { name: 'Your YouTube channels' }));
}

function twitch() {
  return within(screen.getByRole('group', { name: 'Your Twitch channels' }));
}

function patches(calls: { cmd: string; args: Record<string, unknown> }[]) {
  return calls
    .filter((c) => c.cmd === 'set_app_settings')
    .map((c) => (c.args.request as MutationRequest<SettingsPatch>).payload);
}

describe('FeedSettings', () => {
  it('shows the defaults and saves each change as a feed patch', async () => {
    const calls = setup();
    expect(screen.getByRole('switch', { name: /Show the feed/ })).toBeChecked();
    expect(
      screen.getByRole('switch', { name: 'Snapshots, pre-releases and release candidates' }),
    ).toBeChecked();
    expect(screen.getByRole('switch', { name: 'YouTube Shorts' })).not.toBeChecked();
    expect(
      screen.getByRole('switch', { name: /Machine-translated patch notes and news/ }),
    ).toBeChecked();
    // Off until the player asks for it.
    expect(
      screen.getByRole('switch', { name: /Tell me when a favourite streamer goes live/ }),
    ).not.toBeChecked();
    expect(screen.getByRole('radio', { name: 'In a launcher window' })).toBeChecked();
    expect(screen.getByRole('combobox', { name: 'Language of videos and streams' })).toHaveValue(
      'ui',
    );
    const hide = screen.getByRole('group', { name: 'Hide in the feed' });
    for (const box of within(hide).getAllByRole('checkbox')) expect(box).not.toBeChecked();

    fireEvent.click(screen.getByRole('switch', { name: 'YouTube Shorts' }));
    await waitFor(() => {
      expect(patches(calls)).toEqual([{ feed: { shorts: true } }]);
    });
    fireEvent.click(screen.getByRole('radio', { name: 'In the browser' }));
    await waitFor(() => {
      expect(patches(calls).at(-1)).toEqual({ feed: { player: 'browser' } });
    });
    fireEvent.change(screen.getByRole('combobox', { name: 'Language of videos and streams' }), {
      target: { value: 'ar' },
    });
    await waitFor(() => {
      expect(patches(calls).at(-1)).toEqual({ feed: { videoLanguage: 'ar' } });
    });
    fireEvent.click(within(hide).getByRole('checkbox', { name: 'Streams' }));
    await waitFor(() => {
      expect(patches(calls).at(-1)).toEqual({ feed: { hidden: ['streams'] } });
    });
    fireEvent.click(
      screen.getByRole('switch', { name: /Tell me when a favourite streamer goes live/ }),
    );
    await waitFor(() => {
      expect(patches(calls).at(-1)).toEqual({ feed: { liveNotifications: true } });
    });
    fireEvent.click(screen.getByRole('switch', { name: /Show the feed/ }));
    await waitFor(() => {
      expect(patches(calls).at(-1)).toEqual({ feed: { enabled: false } });
    });
  });

  it('adds and removes own channels', async () => {
    const calls = setup();
    expect(await youtube().findByText('No channels added yet.')).toBeVisible();
    const input = youtube().getByRole('textbox', { name: 'Channel link, @handle or channel ID' });
    fireEvent.change(input, { target: { value: '  @mine  ' } });
    fireEvent.click(youtube().getByRole('button', { name: 'Add' }));
    expect(await youtube().findByText('Mine')).toBeVisible();
    expect(calls.find((c) => c.cmd === 'feed_add_channel')?.args).toEqual({ input: '@mine' });
    expect(input).toHaveValue('');
    expect(youtube().getByText('1 of 30')).toBeVisible();
    // The Twitch list is apart.
    expect(twitch().getByText('No channels added yet.')).toBeVisible();
    fireEvent.click(youtube().getByRole('button', { name: 'Remove Mine' }));
    expect(await youtube().findByText('No channels added yet.')).toBeVisible();
    expect(calls.find((c) => c.cmd === 'feed_remove_channel')?.args).toEqual({
      channelId: mine.channelId,
    });
  });

  it('adds and removes own Twitch channels', async () => {
    const calls = setup();
    expect(await twitch().findByText('No channels added yet.')).toBeVisible();
    const input = twitch().getByRole('textbox', { name: 'Channel link or name' });
    fireEvent.change(input, { target: { value: ' https://www.twitch.tv/ph1lza ' } });
    fireEvent.click(twitch().getByRole('button', { name: 'Add' }));
    expect(await twitch().findByText('Ph1LzA')).toBeVisible();
    expect(calls.find((c) => c.cmd === 'feed_add_streamer')?.args).toEqual({
      input: 'https://www.twitch.tv/ph1lza',
    });
    expect(input).toHaveValue('');
    expect(youtube().getByText('No channels added yet.')).toBeVisible();
    fireEvent.click(twitch().getByRole('button', { name: 'Remove Ph1LzA' }));
    expect(await twitch().findByText('No channels added yet.')).toBeVisible();
    expect(calls.find((c) => c.cmd === 'feed_remove_streamer')?.args).toEqual({ login: 'ph1lza' });
  });

  it('says in words why a Twitch channel was not added', async () => {
    for (const [problem, text] of [
      ['bad_input', 'This is not a Twitch channel link or name.'],
      ['not_found', 'There is no such channel on Twitch.'],
      ['too_many', 'Too many channel lookups for now. Try again in a minute.'],
    ] as const) {
      setup(undefined, () => {
        // eslint-disable-next-line @typescript-eslint/only-throw-error
        throw problem;
      });
      const input = twitch().getByRole('textbox', { name: 'Channel link or name' });
      fireEvent.change(input, { target: { value: '<img src=x onerror=alert(1)>' } });
      fireEvent.click(twitch().getByRole('button', { name: 'Add' }));
      expect(await twitch().findByRole('alert')).toHaveTextContent(text);
      expect(input).toHaveValue('<img src=x onerror=alert(1)>');
      document.body.innerHTML = '';
    }
  });

  it('says in words why a channel was not added', async () => {
    for (const [problem, text] of [
      ['bad_input', 'This is not a YouTube channel link, @handle or channel ID.'],
      ['not_found', 'This is no longer in the feed.'],
      ['too_many', 'Too many channel lookups for now. Try again in a minute.'],
      ['full', 'You can add up to 30 channels. Remove one first.'],
    ] as const) {
      const calls = setup(() => {
        // The core rejects with the problem's name, a plain string.
        // eslint-disable-next-line @typescript-eslint/only-throw-error
        throw problem;
      });
      const input = youtube().getByRole('textbox', { name: 'Channel link, @handle or channel ID' });
      fireEvent.change(input, { target: { value: "<script>alert(1)</script>' OR 1=1" } });
      fireEvent.click(youtube().getByRole('button', { name: 'Add' }));
      expect(await screen.findByRole('alert')).toHaveTextContent(text);
      // What was typed stays, to correct it.
      expect(input).toHaveValue("<script>alert(1)</script>' OR 1=1");
      expect(calls.filter((c) => c.cmd === 'feed_add_channel')).toHaveLength(1);
      document.body.innerHTML = '';
    }
  });

  it('sends nothing for an empty channel', async () => {
    const calls = setup();
    fireEvent.click(youtube().getByRole('button', { name: 'Add' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('not a YouTube channel link');
    expect(calls.some((c) => c.cmd === 'feed_add_channel')).toBe(false);
  });

  it('lists the channels shown first and the hidden ones, and takes them off', async () => {
    const calls = setup(undefined, undefined, {
      hidden: [{ kind: 'twitch', id: 'zubarefff_1', name: '<b>Copy</b>' }],
      favorite: [{ kind: 'youtube', id: 'UCabcdefghijklmnopqrstuv', name: 'Loved' }],
    });
    const first = within(screen.getByRole('group', { name: 'Shown first' }));
    const hidden = within(screen.getByRole('group', { name: 'Not shown' }));
    expect(await first.findByText('Loved')).toBeVisible();
    expect(first.getByText('YouTube')).toBeVisible();
    // A name is text, never markup.
    expect(hidden.getByText('<b>Copy</b>')).toBeVisible();
    fireEvent.click(hidden.getByRole('button', { name: 'Remove <b>Copy</b>' }));
    expect(await hidden.findByText('None yet.')).toBeVisible();
    expect(calls.find((c) => c.cmd === 'feed_mark')?.args).toEqual({
      kind: 'twitch',
      id: 'zubarefff_1',
      name: '<b>Copy</b>',
      list: null,
    });
    expect(first.getByText('Loved')).toBeVisible();
  });
});
