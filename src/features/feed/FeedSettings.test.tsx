import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it } from 'vitest';
import type { MutationRequest, OwnChannel, SettingsPatch } from '../../lib/contracts';
import { snapshotFixture } from '../../test/fixtures';
import { FeedSettings } from './FeedSettings';

const mine: OwnChannel = { channelId: 'UCabcdefghijklmnopqrstuv', title: 'Mine' };

function setup(add: (input: string) => OwnChannel[] = () => [mine]) {
  const calls: { cmd: string; args: Record<string, unknown> }[] = [];
  let channels: OwnChannel[] = [];
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
      case 'set_app_settings':
        return snapshotFixture().settings;
      default:
        return null;
    }
  });
  render(<FeedSettings settings={snapshotFixture().settings} />);
  return calls;
}

function patches(calls: { cmd: string; args: Record<string, unknown> }[]) {
  return calls
    .filter((c) => c.cmd === 'set_app_settings')
    .map((c) => (c.args.request as MutationRequest<SettingsPatch>).payload);
}

describe('FeedSettings', () => {
  it('shows the defaults and saves each change as a feed patch', async () => {
    const calls = setup();
    expect(screen.getByRole('checkbox', { name: /Show the feed/ })).toBeChecked();
    expect(
      screen.getByRole('checkbox', { name: 'Snapshots, pre-releases and release candidates' }),
    ).toBeChecked();
    expect(screen.getByRole('checkbox', { name: 'YouTube Shorts' })).not.toBeChecked();
    expect(
      screen.getByRole('checkbox', { name: /Machine-translated patch notes and news/ }),
    ).toBeChecked();
    expect(screen.getByRole('radio', { name: 'In a launcher window' })).toBeChecked();
    expect(screen.getByRole('combobox', { name: 'Language of videos and streams' })).toHaveValue(
      'ui',
    );
    const hide = screen.getByRole('group', { name: 'Hide in the feed' });
    for (const box of within(hide).getAllByRole('checkbox')) expect(box).not.toBeChecked();

    fireEvent.click(screen.getByRole('checkbox', { name: 'YouTube Shorts' }));
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
    fireEvent.click(screen.getByRole('checkbox', { name: /Show the feed/ }));
    await waitFor(() => {
      expect(patches(calls).at(-1)).toEqual({ feed: { enabled: false } });
    });
  });

  it('adds and removes own channels', async () => {
    const calls = setup();
    expect(await screen.findByText('No channels added yet.')).toBeVisible();
    const input = screen.getByRole('textbox', { name: 'Channel link, @handle or channel ID' });
    fireEvent.change(input, { target: { value: '  @mine  ' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add' }));
    expect(await screen.findByText('Mine')).toBeVisible();
    expect(calls.find((c) => c.cmd === 'feed_add_channel')?.args).toEqual({ input: '@mine' });
    expect(input).toHaveValue('');
    expect(screen.getByText('1 of 30')).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Remove Mine' }));
    expect(await screen.findByText('No channels added yet.')).toBeVisible();
    expect(calls.find((c) => c.cmd === 'feed_remove_channel')?.args).toEqual({
      channelId: mine.channelId,
    });
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
      const input = screen.getByRole('textbox', { name: 'Channel link, @handle or channel ID' });
      fireEvent.change(input, { target: { value: "<script>alert(1)</script>' OR 1=1" } });
      fireEvent.click(screen.getByRole('button', { name: 'Add' }));
      expect(await screen.findByRole('alert')).toHaveTextContent(text);
      // What was typed stays, to correct it.
      expect(input).toHaveValue("<script>alert(1)</script>' OR 1=1");
      expect(calls.filter((c) => c.cmd === 'feed_add_channel')).toHaveLength(1);
      document.body.innerHTML = '';
    }
  });

  it('sends nothing for an empty channel', async () => {
    const calls = setup();
    fireEvent.click(screen.getByRole('button', { name: 'Add' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('not a YouTube channel link');
    expect(calls.some((c) => c.cmd === 'feed_add_channel')).toBe(false);
  });
});
