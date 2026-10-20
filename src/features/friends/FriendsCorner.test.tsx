import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { emit } from '@tauri-apps/api/event';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it, vi } from 'vitest';
import type { AppSnapshot, ChatMessage, ChatRoom, FriendsView } from '../../lib/contracts';
import { instanceFixture, snapshotFixture } from '../../test/fixtures';
import { FriendsCorner } from './FriendsCorner';

const ME = { id: '00000000-0000-4000-8000-000000000001', nick: 'Steve_2' };
const ALEX = { id: '00000000-0000-4000-8000-000000000002', nick: 'Alex' };
const NOTCH = { id: '00000000-0000-4000-8000-000000000003', nick: 'Notch_1' };
const ROOM = '10000000-0000-4000-8000-000000000001';

const HOSTILE = '<img src=x onerror="alert(1)"><script>alert(2)</script>‮gnp.exe';

function message(id: number, sender: string, body: string): ChatMessage {
  return { id, room: ROOM, sender, body, created: 1_790_000_000 + id };
}

function direct(overrides: Partial<ChatRoom> = {}): ChatRoom {
  return {
    id: ROOM,
    kind: 'direct',
    name: '',
    owner: ME.id,
    members: [ME, ALEX],
    last: message(2, ALEX.id, HOSTILE),
    unread: 1,
    ...overrides,
  };
}

function signedIn(overrides: Partial<FriendsView> = {}): FriendsView {
  return {
    signedIn: true,
    connected: true,
    me: ME,
    friends: [
      { ...ALEX, at: 1, online: true, playing: 'Minecraft 1.21.1 · Fabric' },
      { ...NOTCH, at: 2, online: false, playing: null },
    ],
    incoming: [],
    outgoing: [],
    blocked: [],
    rooms: [direct()],
    unread: 1,
    visible: true,
    sanction: null,
    problem: null,
    ...overrides,
  };
}

const signedOut: FriendsView = {
  signedIn: false,
  connected: false,
  me: null,
  friends: [],
  incoming: [],
  outgoing: [],
  blocked: [],
  rooms: [],
  unread: 0,
  visible: true,
  sanction: null,
  problem: null,
};

/** Answers friends commands; a function answer may throw a problem string. */
function setup(
  view: FriendsView,
  answers: Record<string, (args: unknown) => unknown> = {},
  snapshot: AppSnapshot = snapshotFixture(),
) {
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC(
    (cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === 'friends_state') return view;
      const answer = answers[cmd];
      if (answer !== undefined) return answer(args);
      if (cmd === 'friends_messages')
        return [message(1, ME.id, 'hi'), message(2, ALEX.id, HOSTILE)];
      return null;
    },
    { shouldMockEvents: true },
  );
  const onOpenAccounts = vi.fn();
  const onOpenWorld = vi.fn();
  render(
    <FriendsCorner snapshot={snapshot} onOpenAccounts={onOpenAccounts} onOpenWorld={onOpenWorld} />,
  );
  return { calls, onOpenAccounts, onOpenWorld };
}

async function openPanel() {
  fireEvent.click(await screen.findByRole('button', { name: /Friends and chats/ }));
  return screen.getByRole('region', { name: 'Friends' });
}

describe('friends corner', () => {
  it('sends a signed-out player to Accounts', async () => {
    const { onOpenAccounts } = setup(signedOut);
    const panel = await openPanel();
    expect(within(panel).getByText(/need a launcher account/)).toBeVisible();
    fireEvent.click(within(panel).getByRole('button', { name: 'Open Accounts' }));
    expect(onOpenAccounts).toHaveBeenCalled();
  });

  it('shows unread messages and requests on the button', async () => {
    setup(signedIn({ incoming: [{ ...NOTCH, at: 3, online: false, playing: null }] }));
    const button = await screen.findByRole('button', { name: /1 unread message/ });
    expect(within(button).getByText('2')).toBeVisible();
  });

  it('shows what players wrote as text, never as markup', async () => {
    const { calls } = setup(signedIn());
    const panel = await openPanel();
    // The preview in the chat list.
    expect(within(panel).getByText(/<img src=x onerror/)).toBeVisible();
    fireEvent.click(within(panel).getByRole('button', { name: /Alex/ }));
    const history = await screen.findByRole('list', { name: 'Alex' });
    await waitFor(() => {
      expect(within(history).getByText(HOSTILE)).toBeVisible();
    });
    expect(document.querySelector('img')).toBeNull();
    expect(document.querySelector('script')).toBeNull();
    expect(within(history).getByText(HOSTILE)).toHaveAttribute('dir', 'auto');
    // Seen on screen: read up to the newest message.
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'friends_read')?.args).toEqual({ room: ROOM, upto: 2 });
    });
  });

  it('sends with Enter, keeps Shift+Enter for a new line', async () => {
    const { calls } = setup(signedIn(), {
      friends_send: (args) => message(3, ME.id, (args as { body: string }).body),
    });
    const panel = await openPanel();
    fireEvent.click(within(panel).getByRole('button', { name: /Alex/ }));
    const box = await screen.findByRole('textbox', { name: 'Write a message' });
    fireEvent.change(box, { target: { value: '  line one\n${7*7}  ' } });
    fireEvent.keyDown(box, { key: 'Enter', shiftKey: true });
    expect(calls.some((c) => c.cmd === 'friends_send')).toBe(false);
    fireEvent.keyDown(box, { key: 'Enter' });
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'friends_send')?.args).toEqual({
        room: ROOM,
        body: 'line one\n${7*7}',
      });
    });
    await waitFor(() => {
      expect(box).toHaveValue('');
    });
    expect(screen.getByText(/\$\{7\*7\}/)).toBeVisible();
  });

  it('adds a message that arrives while the chat is open', async () => {
    setup(signedIn());
    const panel = await openPanel();
    fireEvent.click(within(panel).getByRole('button', { name: /Alex/ }));
    const history = await screen.findByRole('list', { name: 'Alex' });
    await waitFor(() => {
      expect(within(history).getByText('hi')).toBeVisible();
    });
    await act(async () => {
      await emit('friends://message', message(9, ALEX.id, 'are you there?'));
      // Another room's message stays out of this one.
      await emit('friends://message', { ...message(10, ALEX.id, 'elsewhere'), room: 'other' });
    });
    expect(within(history).getByText('are you there?')).toBeVisible();
    expect(within(history).queryByText('elsewhere')).toBeNull();
  });

  it('cannot write to someone who is no longer a friend', async () => {
    setup(signedIn({ friends: [{ ...NOTCH, at: 2, online: false, playing: null }] }));
    const panel = await openPanel();
    fireEvent.click(within(panel).getByRole('button', { name: /Alex/ }));
    expect(await screen.findByText(/no longer friends/)).toBeVisible();
    expect(screen.queryByRole('textbox', { name: 'Write a message' })).toBeNull();
  });

  it('asks by nick and says why a nick is refused', async () => {
    const { calls } = setup(signedIn(), {
      friends_request: (args) => {
        const nick = (args as { nick: string }).nick;
        if (nick === 'Alex') return 'friends';
        // The core rejects with the problem's name, a plain string.
        // eslint-disable-next-line @typescript-eslint/only-throw-error
        throw 'no_such_nick';
      },
    });
    const panel = await openPanel();
    fireEvent.click(within(panel).getByRole('tab', { name: 'Friends' }));
    const input = within(panel).getByRole('textbox', { name: 'Add a friend by nick' });
    fireEvent.change(input, { target: { value: "x' OR 1=1; --" } });
    fireEvent.click(within(panel).getByRole('button', { name: 'Send request' }));
    expect(await within(panel).findByRole('alert')).toHaveTextContent(
      'There is no one with this nick.',
    );
    expect(calls.find((c) => c.cmd === 'friends_request')?.args).toEqual({ nick: "x' OR 1=1; --" });
    fireEvent.change(input, { target: { value: ' Alex ' } });
    fireEvent.click(within(panel).getByRole('button', { name: 'Send request' }));
    expect(await within(panel).findByText('You and Alex are friends now.')).toBeVisible();
  });

  it('blocks only after confirming', async () => {
    const { calls } = setup(signedIn());
    const panel = await openPanel();
    fireEvent.click(within(panel).getByRole('tab', { name: 'Friends' }));
    const row = within(panel).getByText('Notch_1').closest('li') as HTMLElement;
    fireEvent.click(within(row).getByRole('button', { name: 'Block' }));
    expect(calls.some((c) => c.cmd === 'friends_act')).toBe(false);
    const confirm = within(panel).getByRole('alertdialog');
    expect(confirm).toHaveTextContent('Block Notch_1? They will not be told.');
    fireEvent.click(within(confirm).getByRole('button', { name: 'Block' }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'friends_act')?.args).toEqual({
        action: 'block',
        id: NOTCH.id,
      });
    });
  });

  it('accepts, declines and cancels requests', async () => {
    const { calls } = setup(
      signedIn({
        incoming: [{ ...NOTCH, at: 3, online: false, playing: null }],
        outgoing: [{ id: 'x', nick: 'Herobrine', at: 4, online: false, playing: null }],
      }),
    );
    const panel = await openPanel();
    fireEvent.click(within(panel).getByRole('tab', { name: /Requests/ }));
    fireEvent.click(within(panel).getByRole('button', { name: 'Accept' }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'friends_act')?.args).toEqual({
        action: 'accept',
        id: NOTCH.id,
      });
    });
    fireEvent.click(within(panel).getByRole('button', { name: 'Cancel' }));
    await waitFor(() => {
      expect(calls.filter((c) => c.cmd === 'friends_act')[1]?.args).toEqual({
        action: 'cancel',
        id: 'x',
      });
    });
  });

  it('makes a group of at least two friends with a name', async () => {
    const { calls } = setup(signedIn(), {
      friends_create_group: () => '20000000-0000-4000-8000-000000000002',
    });
    const panel = await openPanel();
    fireEvent.click(within(panel).getByRole('button', { name: 'New group' }));
    const create = within(panel).getByRole('button', { name: 'Create' });
    expect(create).toBeDisabled();
    fireEvent.change(within(panel).getByRole('textbox', { name: 'Group name' }), {
      target: { value: 'Builders' },
    });
    fireEvent.click(within(panel).getByRole('checkbox', { name: 'Alex' }));
    expect(create).toBeDisabled();
    fireEvent.click(within(panel).getByRole('checkbox', { name: 'Notch_1' }));
    fireEvent.click(create);
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'friends_create_group')?.args).toEqual({
        name: 'Builders',
        members: [ALEX.id, NOTCH.id],
      });
    });
  });

  it('leaves a group only after confirming', async () => {
    const group = direct({
      kind: 'group',
      name: 'Builders',
      owner: ALEX.id,
      members: [ME, ALEX, NOTCH],
    });
    const { calls } = setup(signedIn({ rooms: [group] }));
    const panel = await openPanel();
    fireEvent.click(within(panel).getByRole('button', { name: /Builders/ }));
    fireEvent.click(await within(panel).findByRole('button', { name: /3 people/ }));
    fireEvent.click(within(panel).getByRole('button', { name: 'Leave the group' }));
    const asked = within(panel).getByRole('alertdialog');
    fireEvent.click(within(asked).getByRole('button', { name: 'Cancel' }));
    expect(calls.some((c) => c.cmd === 'friends_change_room')).toBe(false);
    fireEvent.click(within(panel).getByRole('button', { name: 'Leave the group' }));
    fireEvent.click(
      within(within(panel).getByRole('alertdialog')).getByRole('button', {
        name: 'Leave the group',
      }),
    );
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'friends_change_room')?.args).toEqual({
        room: ROOM,
        action: 'remove',
        value: ME.id,
      });
    });
  });

  it('shows who is online and what they play, and hides me when asked', async () => {
    const { calls } = setup(signedIn());
    const panel = await openPanel();
    fireEvent.click(within(panel).getByRole('tab', { name: 'Friends' }));
    expect(within(panel).getByText('Playing Minecraft 1.21.1 · Fabric')).toBeVisible();
    expect(within(panel).queryAllByText('Online')).toHaveLength(0);
    const show = within(panel).getByRole('checkbox', { name: 'Show friends that I am online' });
    expect(show).toBeChecked();
    fireEvent.click(show);
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'friends_set_visible')?.args).toEqual({
        visible: false,
      });
    });
  });

  it('says when a moderator muted me and offers no box to write in', async () => {
    setup(signedIn({ sanction: { kind: 'mute', until: 0 } }));
    const panel = await openPanel();
    expect(within(panel).getByText('You cannot write: a moderator muted you.')).toBeVisible();
    fireEvent.click(within(panel).getByRole('button', { name: /Alex/ }));
    await screen.findByRole('list', { name: 'Alex' });
    expect(within(panel).queryByRole('textbox', { name: 'Write a message' })).toBeNull();
  });

  it('takes a message a moderator removed out of the open chat', async () => {
    setup(signedIn());
    const panel = await openPanel();
    fireEvent.click(within(panel).getByRole('button', { name: /Alex/ }));
    const history = await screen.findByRole('list', { name: 'Alex' });
    await waitFor(() => {
      expect(within(history).getByText(HOSTILE)).toBeVisible();
    });
    await act(async () => {
      await emit('friends://removed', { room: ROOM, message: 2 });
    });
    await waitFor(() => {
      expect(within(history).queryByText(HOSTILE)).toBeNull();
    });
    expect(within(history).getByText('hi')).toBeVisible();
  });

  it('reports a request as spam or declines them all', async () => {
    const { calls } = setup(
      signedIn({
        incoming: [
          { ...NOTCH, at: 3, online: false, playing: null },
          { id: 'y', nick: 'buy_gold_cheap', at: 4, online: false, playing: null },
        ],
      }),
    );
    const panel = await openPanel();
    fireEvent.click(within(panel).getByRole('tab', { name: /Requests/ }));
    const spam = within(panel).getAllByRole('button', { name: 'Spam' });
    fireEvent.click(spam[1] as HTMLElement);
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'friends_report_request')?.args).toEqual({ id: 'y' });
    });
    fireEvent.click(within(panel).getByRole('button', { name: 'Decline all' }));
    await waitFor(() => {
      expect(calls.some((c) => c.cmd === 'friends_decline_all')).toBe(true);
    });
  });

  it('follows state the core sends', async () => {
    setup(signedOut);
    const panel = await openPanel();
    expect(within(panel).getByText(/need a launcher account/)).toBeVisible();
    await act(async () => {
      await emit('friends://state', signedIn({ connected: false }));
    });
    expect(within(panel).getByText('Not connected. Trying again…')).toBeVisible();
    expect(within(panel).getByRole('tab', { name: /Chats/ })).toBeVisible();
  });

  it('says to confirm the email, not to sign in, and checks again', async () => {
    const { calls } = setup(
      { ...signedOut, signedIn: true, problem: 'email_not_confirmed' },
      { launcher_account_recheck: () => false },
    );
    const panel = await openPanel();
    expect(within(panel).getByText(/once your email is confirmed/)).toBeVisible();
    expect(within(panel).queryByText(/need a launcher account/)).toBeNull();
    fireEvent.click(within(panel).getByRole('button', { name: 'Check again' }));
    expect(await within(panel).findByText(/Not confirmed yet/)).toBeVisible();
    fireEvent.click(within(panel).getByRole('button', { name: 'Send the letter again' }));
    expect(await within(panel).findByText(/The letter is on its way/)).toBeVisible();
    expect(calls.map((c) => c.cmd)).toEqual(
      expect.arrayContaining(['launcher_account_recheck', 'launcher_account_resend_confirmation']),
    );
  });

  it('lets a report be closed without sending it', async () => {
    const { calls } = setup(signedIn());
    const panel = await openPanel();
    fireEvent.click(within(panel).getByRole('button', { name: /Alex/ }));
    fireEvent.click(await within(panel).findByRole('button', { name: 'Report' }));
    fireEvent.click(within(panel).getByRole('button', { name: 'Cancel' }));
    expect(within(panel).queryByRole('textbox', { name: 'What is wrong with it?' })).toBeNull();
    // Escape closes the report, and only the report.
    fireEvent.click(within(panel).getByRole('button', { name: 'Report' }));
    fireEvent.keyDown(within(panel).getByRole('textbox', { name: 'What is wrong with it?' }), {
      key: 'Escape',
    });
    expect(within(panel).queryByRole('textbox', { name: 'What is wrong with it?' })).toBeNull();
    expect(screen.getByRole('region', { name: 'Friends' })).toBeVisible();
    expect(calls.some((c) => c.cmd === 'friends_report')).toBe(false);
  });

  it('keeps unread chats apart and marks where new messages begin', async () => {
    const OTHER = '10000000-0000-4000-8000-000000000002';
    setup(
      signedIn({
        rooms: [
          direct(),
          direct({
            id: OTHER,
            members: [ME, NOTCH],
            last: { ...message(1, NOTCH.id, 'old'), room: OTHER },
            unread: 0,
          }),
        ],
      }),
    );
    const panel = await openPanel();
    const unread = within(panel).getByRole('heading', { name: 'Unread' });
    const read = within(panel).getByRole('heading', { name: 'Read' });
    expect(unread.compareDocumentPosition(read) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    fireEvent.click(within(panel).getByRole('button', { name: /Alex/ }));
    const divider = await within(panel).findByRole('separator');
    expect(divider).toHaveTextContent('New messages');
    // Before Alex's message, after mine.
    expect(divider.nextElementSibling).toHaveTextContent('Alex');
    expect(divider.previousElementSibling).toHaveTextContent('hi');
  });

  it('shows that messages are loading', async () => {
    setup(signedIn(), { friends_messages: () => new Promise(() => undefined) });
    const panel = await openPanel();
    fireEvent.click(within(panel).getByRole('button', { name: /Alex/ }));
    expect(await within(panel).findByText('Loading messages…')).toBeVisible();
  });

  it('turns the sound and system notifications off', async () => {
    const { calls } = setup(signedIn());
    const panel = await openPanel();
    fireEvent.click(within(panel).getByRole('tab', { name: /Friends/ }));
    fireEvent.click(within(panel).getByRole('checkbox', { name: /Sound for new messages/ }));
    fireEvent.click(within(panel).getByRole('checkbox', { name: /System notifications/ }));
    await waitFor(() => {
      expect(
        calls
          .filter((c) => c.cmd === 'set_app_settings')
          .map((c) => (c.args as { request: { payload: unknown } }).request.payload),
      ).toEqual([{ chatSound: false }, { chatNotifications: false }]);
    });
  });

  it('invites a friend to the server the game is on', async () => {
    const instance = instanceFixture({ versionId: '1.21.1' });
    const playing = snapshotFixture({
      instances: [instance],
      currentRun: {
        runId: 'r1',
        kind: 'game',
        instanceId: instance.id,
        state: 'running',
        startedAt: '2026-10-19T12:00:00Z',
        endedAt: null,
        exitCode: null,
        durationMs: null,
        drainTimedOut: false,
        outputTail: [],
        outputLinesDropped: 0,
        error: null,
        loggingDegraded: false,
        signal: null,
        diagnosis: null,
        server: 'play.example.com:25565',
      },
    });
    const { calls } = setup(
      signedIn(),
      {
        friends_send: (args) => ({
          ...message(9, ME.id, (args as { body: string }).body),
        }),
        friends_open_direct: () => ROOM,
      },
      playing,
    );
    const panel = await openPanel();
    fireEvent.click(within(panel).getByRole('tab', { name: /Friends/ }));
    fireEvent.click(within(panel).getAllByRole('button', { name: 'Invite' })[0] as HTMLElement);
    expect(await within(panel).findByText('Invite sent to Alex.')).toBeVisible();
    expect(calls.find((c) => c.cmd === 'friends_send')?.args).toEqual({
      room: ROOM,
      body: '🎮 invite play.example.com:25565 1.21.1',
    });
  });

  it('shows an invite as a card and joins with an installation of that version', async () => {
    const old = instanceFixture({
      id: 'old',
      versionId: '1.20.4',
      displayName: 'Old',
      health: 'ready',
    });
    const match = instanceFixture({
      id: 'match',
      versionId: '1.21.1',
      displayName: 'Survival',
      health: 'ready',
    });
    const { calls } = setup(
      signedIn(),
      {
        friends_messages: () => [
          message(1, ALEX.id, '🎮 invite play.example.com:25565 1.21.1'),
          message(2, ALEX.id, '🎮 invite evil host; --demo 1.21.1'),
        ],
      },
      snapshotFixture({ instances: [old, match] }),
    );
    const panel = await openPanel();
    fireEvent.click(within(panel).getByRole('button', { name: /Alex/ }));
    expect(await within(panel).findByText('Alex invites you to a server')).toBeVisible();
    expect(within(panel).getByText('play.example.com:25565')).toBeVisible();
    // Not a valid invite: plain text, no button.
    expect(within(panel).getByText('🎮 invite evil host; --demo 1.21.1')).toBeVisible();
    fireEvent.click(within(panel).getByRole('button', { name: 'Join' }));
    await waitFor(() => {
      const launch = calls.find((c) => c.cmd === 'launch_game')?.args as {
        request: { payload: unknown };
      };
      expect(launch.request.payload).toEqual({
        instanceId: 'match',
        server: 'play.example.com:25565',
      });
    });
  });

  it('says what is missing to join', async () => {
    setup(
      signedIn(),
      { friends_messages: () => [message(1, ALEX.id, '🎮 invite play.example.com:25565 1.7.10')] },
      snapshotFixture({ instances: [instanceFixture({ versionId: '1.21.1', health: 'ready' })] }),
    );
    const panel = await openPanel();
    fireEvent.click(within(panel).getByRole('button', { name: /Alex/ }));
    expect(
      await within(panel).findByText(/You have no installation of Minecraft 1.7.10/),
    ).toBeVisible();
    expect(within(panel).queryByRole('button', { name: 'Join' })).toBeNull();
  });

  it('looks at a world invite first, then joins the room and opens it', async () => {
    const code = 'AbCdEfGhIjKlMnOpQrStUv';
    const { calls, onOpenWorld } = setup(signedIn(), {
      friends_messages: () => [
        message(1, ALEX.id, `🌍 world ${code}`),
        message(2, ALEX.id, '🌍 world ../../v1/rooms'),
      ],
      worlds_preview: () => ({
        room: ROOM,
        name: 'Our base',
        ownerNick: 'Alex',
        members: 2,
        minecraft: '1.21.1',
      }),
      worlds_redeem: () => ({ id: ROOM, name: 'Our base' }),
    });
    const panel = await openPanel();
    fireEvent.click(within(panel).getByRole('button', { name: /Alex/ }));
    expect(await within(panel).findByText('Alex invites you to a world')).toBeVisible();
    // Not a code: plain text, no card.
    expect(within(panel).getByText('🌍 world ../../v1/rooms')).toBeVisible();
    expect(calls.some((c) => c.cmd === 'worlds_redeem')).toBe(false);
    fireEvent.click(within(panel).getByRole('button', { name: 'Look' }));
    expect(await within(panel).findByText(/Our base, by Alex/)).toBeVisible();
    fireEvent.click(within(panel).getByRole('button', { name: 'Join the room' }));
    await waitFor(() => {
      expect(onOpenWorld).toHaveBeenCalledWith(ROOM);
    });
    expect(calls.find((c) => c.cmd === 'worlds_redeem')?.args).toEqual({ code });
  });

  it('says which world a friend plays in', async () => {
    setup(
      signedIn({
        friends: [{ ...ALEX, at: 1, online: true, playing: '🌍 Our base' }],
      }),
    );
    const panel = await openPanel();
    fireEvent.click(within(panel).getByRole('tab', { name: /Friends/ }));
    expect(await within(panel).findByText('In the world Our base')).toBeVisible();
  });
});
