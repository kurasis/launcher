import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { emit } from '@tauri-apps/api/event';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it, vi } from 'vitest';
import type { AppSnapshot, ChatMessage, ChatRoom, FriendsView } from '../../lib/contracts';
import { Dialog } from '../../components/Dialog';
import { addDialog, removeDialog } from '../../components/dialogStack';
import { instanceFixture, snapshotFixture } from '../../test/fixtures';
import { FriendsCorner } from './FriendsCorner';
import { playChime } from './sound';

vi.mock('./sound', async (importOriginal) => ({
  ...(await importOriginal<typeof import('./sound')>()),
  playChime: vi.fn(),
}));

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

/** A press as a browser makes it: the button takes the focus, then the click. */
function press(element: HTMLElement) {
  element.focus();
  fireEvent.click(element);
}

async function openPanel() {
  fireEvent.click(await screen.findByRole('button', { name: /Friends and chats/ }));
  return screen.getByRole('region', { name: 'Friends and chats' });
}

describe('friends corner', () => {
  it('sends a signed-out player to Accounts', async () => {
    const { onOpenAccounts } = setup(signedOut);
    const panel = await openPanel();
    expect(within(panel).getByText(/need a launcher account/)).toBeVisible();
    fireEvent.click(within(panel).getByRole('button', { name: 'Open Accounts' }));
    expect(onOpenAccounts).toHaveBeenCalled();
  });

  it("moves focus into the rail's sheet and back to its button when it closes", async () => {
    setup(signedIn());
    const button = await screen.findByRole('button', { name: /Friends and chats/ });
    fireEvent.click(button);
    const sheet = screen.getByRole('region', { name: 'Friends and chats' });
    expect(within(sheet).getByRole('heading', { name: 'Friends and chats' })).toHaveFocus();
    // The sheet follows its button, so Tab goes from one into the other.
    expect(button.compareDocumentPosition(sheet) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    fireEvent.click(within(sheet).getByRole('button', { name: 'Close' }));
    expect(screen.queryByRole('region', { name: 'Friends and chats' })).toBeNull();
    expect(button).toHaveFocus();
    fireEvent.click(button);
    fireEvent.keyDown(screen.getByRole('heading', { name: 'Friends and chats' }), {
      key: 'Escape',
    });
    expect(screen.queryByRole('region', { name: 'Friends and chats' })).toBeNull();
    expect(button).toHaveFocus();
    // A press on the screen beside the sheet closes it too.
    fireEvent.click(button);
    const scrim = document.querySelector('[aria-hidden="true"][class*="scrim"]');
    fireEvent.click(scrim as HTMLElement);
    expect(screen.queryByRole('region', { name: 'Friends and chats' })).toBeNull();
    expect(button).toHaveFocus();
  });

  it('shows unread messages and requests on the button', async () => {
    setup(signedIn({ incoming: [{ ...NOTCH, at: 3, online: false, playing: null }] }));
    const button = await screen.findByRole('button', { name: /1 unread message/ });
    expect(button).toHaveAccessibleName('Friends and chats, 1 unread message, 1 friend request');
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
    expect(within(panel).getByText('Notch_1')).toBeVisible();
    const menu = within(panel).getByRole('button', { name: 'Chat actions' });
    fireEvent.click(menu);
    fireEvent.click(within(panel).getByRole('menuitem', { name: 'Leave the group' }));
    const asked = within(panel).getByRole('alertdialog');
    expect(asked).toHaveFocus();
    fireEvent.click(within(asked).getByRole('button', { name: 'Cancel' }));
    expect(menu).toHaveFocus();
    expect(calls.some((c) => c.cmd === 'friends_change_room')).toBe(false);
    fireEvent.click(menu);
    fireEvent.click(within(panel).getByRole('menuitem', { name: 'Leave the group' }));
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
    expect(screen.getByRole('region', { name: 'Friends and chats' })).toBeVisible();
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

  it('shows what a world invite leads to, and joins the room only on a press', async () => {
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
        mods: 12,
        hosting: true,
      }),
      worlds_redeem: () => ({ id: ROOM, name: 'Our base' }),
    });
    const panel = await openPanel();
    fireEvent.click(within(panel).getByRole('button', { name: /Alex/ }));
    expect(await within(panel).findByText('Alex invites you to a world')).toBeVisible();
    // Not a code: plain text, no card.
    expect(within(panel).getByText('🌍 world ../../v1/rooms')).toBeVisible();
    // The facts come by themselves; looking joins nothing.
    expect(await within(panel).findByText('Our base, Minecraft 1.21.1, 12 mods')).toBeVisible();
    expect(within(panel).getByText('Open now')).toBeVisible();
    expect(calls.filter((c) => c.cmd === 'worlds_preview').map((c) => c.args)).toEqual([{ code }]);
    expect(calls.some((c) => c.cmd === 'worlds_redeem')).toBe(false);
    fireEvent.click(within(panel).getByRole('button', { name: 'Join the room' }));
    await waitFor(() => {
      expect(onOpenWorld).toHaveBeenCalledWith(ROOM);
    });
    expect(calls.find((c) => c.cmd === 'worlds_redeem')?.args).toEqual({ code });
  });

  it('lets the one who sent a world invite open the room, and says when a code is spent', async () => {
    const mineCode = 'MineMineMineMineMine01';
    const spentCode = 'SpentSpentSpentSpent01';
    const { calls, onOpenWorld } = setup(signedIn(), {
      friends_messages: () => [
        message(1, ME.id, `🌍 world ${mineCode}`),
        message(2, ALEX.id, `🌍 world ${spentCode}`),
      ],
      worlds_preview: (args) => {
        if ((args as { code: string }).code === spentCode) {
          throw Object.assign(new Error('worlds.lapi.sh refused this.'), {
            code: 'INVALID_INPUT',
            retryable: false,
            diagnosticId: 'd-1',
            safeDetails: { worldsProblem: 'invite_invalid' },
          });
        }
        return {
          room: ROOM,
          name: 'Our base',
          ownerNick: 'Steve_2',
          members: 1,
          minecraft: '1.21.1',
          mods: null,
          hosting: false,
        };
      },
    });
    const panel = await openPanel();
    fireEvent.click(within(panel).getByRole('button', { name: /Alex/ }));
    expect(await within(panel).findByText('Our base, Minecraft 1.21.1')).toBeVisible();
    expect(within(panel).queryByText('Open now')).toBeNull();
    // A refused code says so and offers nothing to press.
    expect(
      await within(panel).findByText('This invite code is wrong, used up or expired.'),
    ).toBeVisible();
    expect(within(panel).queryByRole('button', { name: 'Join the room' })).toBeNull();
    fireEvent.click(within(panel).getByRole('button', { name: 'Open room' }));
    expect(onOpenWorld).toHaveBeenCalledWith(ROOM);
    expect(calls.some((c) => c.cmd === 'worlds_redeem')).toBe(false);
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

  it('moves between tabs with the arrow keys, Home and End, and shows counts', async () => {
    setup(signedIn({ incoming: [{ ...NOTCH, at: 3, online: false, playing: null }] }));
    const panel = await openPanel();
    const chats = within(panel).getByRole('tab', { name: 'Chats (1)' });
    expect(chats).toHaveAttribute('aria-selected', 'true');
    expect(chats).toHaveAttribute('tabindex', '0');
    expect(within(panel).getByRole('tab', { name: 'Friends' })).toHaveAttribute('tabindex', '-1');
    fireEvent.keyDown(chats, { key: 'ArrowRight' });
    expect(within(panel).getByRole('tab', { name: 'Friends' })).toHaveFocus();
    expect(within(panel).getByRole('tabpanel')).toHaveAccessibleName('Friends');
    fireEvent.keyDown(within(panel).getByRole('tab', { name: 'Friends' }), { key: 'End' });
    expect(within(panel).getByRole('tab', { name: 'Requests (1)' })).toHaveFocus();
    fireEvent.keyDown(within(panel).getByRole('tab', { name: 'Requests (1)' }), {
      key: 'ArrowRight',
    });
    expect(chats).toHaveFocus();
    // Right to left, the arrows follow the reading direction.
    within(panel).getByRole('tablist').style.direction = 'rtl';
    fireEvent.keyDown(chats, { key: 'ArrowRight' });
    expect(within(panel).getByRole('tab', { name: 'Requests (1)' })).toHaveFocus();
    fireEvent.keyDown(within(panel).getByRole('tab', { name: 'Requests (1)' }), { key: 'Home' });
    expect(chats).toHaveFocus();
  });

  it('offers a new group only with two friends or more, and says why', async () => {
    setup(signedIn({ friends: [{ ...ALEX, at: 1, online: true, playing: null }] }));
    const panel = await openPanel();
    const plus = within(panel).getByRole('button', { name: 'New group' });
    expect(plus).toHaveAttribute('aria-disabled', 'true');
    expect(plus).toHaveAttribute('title', 'A group needs at least two friends.');
    fireEvent.click(plus);
    expect(within(panel).queryByRole('textbox', { name: 'Group name' })).toBeNull();
    // A press says why, to everyone.
    expect(within(panel).getByRole('status')).toHaveTextContent(
      'A group needs at least two friends.',
    );
  });

  it('shows the chosen chat beside the list and keeps the place when opened again', async () => {
    const { calls } = setup(signedIn(), { friends_open_direct: () => ROOM });
    const panel = await openPanel();
    expect(within(panel).getByText('Choose a chat to read it here.')).toBeVisible();
    // Nothing is opened, and so read, by itself.
    expect(calls.some((c) => c.cmd === 'friends_messages')).toBe(false);
    fireEvent.click(within(panel).getByRole('tab', { name: 'Friends' }));
    const row = within(panel).getByText('Alex').closest('li') as HTMLElement;
    fireEvent.click(within(row).getByRole('button', { name: 'Message' }));
    // Message goes to the chat, in the Chats tab.
    expect(await within(panel).findByRole('list', { name: 'Alex' })).toBeVisible();
    expect(within(panel).getByRole('tab', { name: 'Chats (1)' })).toHaveAttribute(
      'aria-selected',
      'true',
    );
    expect(within(panel).getByRole('button', { name: /Alex/ })).toHaveAttribute(
      'aria-current',
      'true',
    );
    fireEvent.click(within(panel).getByRole('button', { name: 'Close' }));
    const again = await openPanel();
    expect(within(again).getByRole('list', { name: 'Alex' })).toBeVisible();
  });

  it('closes only the chat menu with Escape, and blocks after a question', async () => {
    const { calls } = setup(signedIn());
    const panel = await openPanel();
    fireEvent.click(within(panel).getByRole('button', { name: /Alex/ }));
    const menu = await within(panel).findByRole('button', { name: 'Chat actions' });
    fireEvent.click(menu);
    expect(within(panel).getByRole('menuitem', { name: 'Remove from friends' })).toBeVisible();
    fireEvent.keyDown(within(panel).getByRole('menuitem', { name: 'Block' }), { key: 'Escape' });
    expect(within(panel).queryByRole('menu')).toBeNull();
    expect(screen.getByRole('region', { name: 'Friends and chats' })).toBeVisible();
    expect(menu).toHaveFocus();
    fireEvent.click(menu);
    fireEvent.click(within(panel).getByRole('menuitem', { name: 'Block' }));
    const asked = within(panel).getByRole('alertdialog');
    expect(asked).toHaveTextContent('Block Alex? They will not be told.');
    // Escape closes the question, and only the question.
    fireEvent.keyDown(asked, { key: 'Escape' });
    expect(within(panel).queryByRole('alertdialog')).toBeNull();
    expect(screen.getByRole('region', { name: 'Friends and chats' })).toBeVisible();
    expect(calls.some((c) => c.cmd === 'friends_act')).toBe(false);
    fireEvent.click(menu);
    fireEvent.click(within(panel).getByRole('menuitem', { name: 'Block' }));
    fireEvent.click(
      within(within(panel).getByRole('alertdialog')).getByRole('button', { name: 'Block' }),
    );
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'friends_act')?.args).toEqual({
        action: 'block',
        id: ALEX.id,
      });
    });
  });

  it('previews the last message as text: whose it is in a group, mine, invites in words', async () => {
    const GROUP = '10000000-0000-4000-8000-000000000003';
    const OTHER = '10000000-0000-4000-8000-000000000004';
    setup(
      signedIn({
        rooms: [
          direct({ unread: 0, last: message(5, ME.id, 'see you') }),
          direct({
            id: OTHER,
            members: [ME, NOTCH],
            unread: 0,
            last: { ...message(6, NOTCH.id, '🌍 world AbCdEfGhIjKlMnOpQrStUv'), room: OTHER },
          }),
          direct({
            id: GROUP,
            kind: 'group',
            name: 'Builders',
            members: [ME, ALEX, NOTCH],
            unread: 0,
            last: { ...message(7, ALEX.id, HOSTILE), room: GROUP },
          }),
        ],
        unread: 0,
      }),
    );
    const panel = await openPanel();
    expect(within(panel).getByText('You: see you')).toBeVisible();
    expect(within(panel).getByText('Invite to a world')).toBeVisible();
    const group = within(panel).getByText(`Alex: ${HOSTILE}`);
    expect(group).toHaveAttribute('dir', 'auto');
    expect(document.querySelector('script')).toBeNull();
  });

  it('asks for the faces of everyone shown once, and shows only PNG ones', async () => {
    const HEAD = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAgAAAAI';
    // Players of their own: faces are kept between tests as between lists.
    const KIRA = { id: '00000000-0000-4000-8000-000000000007', nick: 'Kira_7' };
    const MAX = { id: '00000000-0000-4000-8000-000000000008', nick: 'Max_7' };
    const { calls } = setup(
      signedIn({
        friends: [
          { ...KIRA, at: 1, online: true, playing: null },
          { ...MAX, at: 2, online: false, playing: null },
        ],
        incoming: [{ ...MAX, at: 3, online: false, playing: null }],
        rooms: [direct({ members: [ME, KIRA], last: null, unread: 0 })],
        unread: 0,
      }),
      { skins_heads: () => ({ Kira_7: HEAD, Max_7: 'javascript:alert(1)' }) },
    );
    const panel = await openPanel();
    await waitFor(() => {
      expect(panel.querySelectorAll('img')).toHaveLength(1);
    });
    expect(panel.querySelector('img')).toHaveAttribute('src', HEAD);
    expect(calls.filter((c) => c.cmd === 'skins_heads').map((c) => c.args)).toEqual([
      { nicks: ['Kira_7', 'Max_7'] },
    ]);
  });

  it('goes away when the focus or a press goes outside it, and Escape works from anywhere', async () => {
    setup(signedIn());
    const outside = document.createElement('button');
    outside.textContent = 'Play';
    document.body.append(outside);
    const sheet = () => screen.queryByRole('region', { name: 'Friends and chats' });
    try {
      const button = await screen.findByRole('button', { name: /Friends and chats/ });
      fireEvent.click(button);
      // Tab out of the sheet: nothing under it is reached with the sheet still over it.
      act(() => {
        outside.focus();
      });
      expect(sheet()).toBeNull();
      expect(outside).toHaveFocus();
      fireEvent.click(button);
      fireEvent.pointerDown(outside);
      expect(sheet()).toBeNull();
      // A lost focus keeps it, and Escape still closes it.
      fireEvent.click(button);
      act(() => {
        (document.activeElement as HTMLElement).blur();
      });
      expect(sheet()).not.toBeNull();
      fireEvent.keyDown(document.body, { key: 'Escape' });
      expect(sheet()).toBeNull();
      expect(button).toHaveFocus();
      // A dialog over it keeps it: its keys and focus are the dialog's.
      fireEvent.click(button);
      const box = document.createElement('div');
      const inside = document.createElement('button');
      box.append(inside);
      document.body.append(box);
      const id = Symbol('dialog');
      addDialog({ id, holds: (node) => box.contains(node), refocus: () => undefined });
      act(() => {
        inside.focus();
      });
      fireEvent.pointerDown(inside);
      fireEvent.keyDown(inside, { key: 'Escape' });
      expect(sheet()).not.toBeNull();
      removeDialog(id);
      box.remove();
    } finally {
      outside.remove();
    }
  });

  it('does not take the player back to a group left while they moved on', async () => {
    const group = direct({
      kind: 'group',
      name: 'Builders',
      owner: ALEX.id,
      members: [ME, ALEX, NOTCH],
    });
    let finish = () => undefined as unknown;
    setup(signedIn({ rooms: [group] }), {
      friends_change_room: () =>
        new Promise((done) => {
          finish = () => {
            done(null);
          };
        }),
    });
    const panel = await openPanel();
    fireEvent.click(within(panel).getByRole('button', { name: /Builders/ }));
    fireEvent.click(await within(panel).findByRole('button', { name: 'Chat actions' }));
    fireEvent.click(within(panel).getByRole('menuitem', { name: 'Leave the group' }));
    const asked = within(panel).getByRole('alertdialog');
    fireEvent.click(within(asked).getByRole('button', { name: 'Leave the group' }));
    // While the answer goes, the question stays.
    const cancel = within(asked).getByRole('button', { name: 'Cancel' });
    await waitFor(() => {
      expect(cancel).toHaveAttribute('aria-disabled', 'true');
    });
    fireEvent.click(cancel);
    fireEvent.keyDown(asked, { key: 'Escape' });
    expect(within(panel).getByRole('alertdialog')).toBe(asked);
    fireEvent.click(within(panel).getByRole('tab', { name: 'Friends' }));
    await act(async () => {
      finish();
      await Promise.resolve();
    });
    expect(within(panel).getByRole('tab', { name: 'Friends' })).toHaveAttribute(
      'aria-selected',
      'true',
    );
  });

  it('chimes for the chat on screen while a new group takes its place', async () => {
    vi.mocked(playChime).mockClear();
    const hasFocus = vi.spyOn(document, 'hasFocus').mockReturnValue(true);
    try {
      setup(signedIn());
      const panel = await openPanel();
      fireEvent.click(within(panel).getByRole('button', { name: /Alex/ }));
      await within(panel).findByRole('list', { name: 'Alex' });
      await act(async () => {
        await emit('friends://message', message(9, ALEX.id, 'on screen'));
      });
      expect(playChime).not.toHaveBeenCalled();
      fireEvent.click(within(panel).getByRole('button', { name: 'New group' }));
      await act(async () => {
        await emit('friends://message', message(10, ALEX.id, 'behind the form'));
      });
      expect(playChime).toHaveBeenCalledTimes(1);
    } finally {
      hasFocus.mockRestore();
    }
  });

  it('keeps the focus in the sheet when what had it goes away', async () => {
    const group = direct({
      kind: 'group',
      name: 'Builders',
      owner: ME.id,
      members: [ME, ALEX, NOTCH],
    });
    let view = signedIn({
      rooms: [group, { ...direct(), id: '10000000-0000-4000-8000-000000000009' }],
    });
    setup(view, {
      friends_state: () => view,
      friends_create_group: () => ROOM,
    });
    const panel = await openPanel();
    // The new group's form: Cancel gives the focus back to "+", Create to the new chat.
    const plus = within(panel).getByRole('button', { name: 'New group' });
    fireEvent.click(plus);
    fireEvent.click(within(panel).getByRole('button', { name: 'Cancel' }));
    expect(plus).toHaveFocus();
    fireEvent.click(plus);
    fireEvent.change(within(panel).getByRole('textbox', { name: 'Group name' }), {
      target: { value: 'Builders' },
    });
    fireEvent.click(within(panel).getByRole('checkbox', { name: 'Alex' }));
    fireEvent.click(within(panel).getByRole('checkbox', { name: 'Notch_1' }));
    fireEvent.click(within(panel).getByRole('button', { name: 'Create' }));
    await waitFor(() => {
      expect(within(panel).getByRole('textbox', { name: 'Write a message' })).toHaveFocus();
    });
    // Renaming: Escape gives the focus back to Rename.
    fireEvent.click(within(panel).getByRole('button', { name: /3 people/ }));
    press(within(panel).getByRole('button', { name: 'Rename' }));
    expect(within(panel).getByRole('textbox', { name: 'Group name' })).toHaveFocus();
    fireEvent.keyDown(within(panel).getByRole('textbox', { name: 'Group name' }), {
      key: 'Escape',
    });
    expect(within(panel).getByRole('button', { name: 'Rename' })).toHaveFocus();
    // A member removed: the next one's button takes the focus.
    const alex = within(panel).getAllByText('Alex').at(-1)?.closest('li') as HTMLElement;
    within(alex).getByRole('button', { name: 'Remove from the group' }).focus();
    view = signedIn({ rooms: [{ ...group, members: [ME, NOTCH] }] });
    await act(async () => {
      await emit('friends://state', view);
    });
    const notch = within(panel).getAllByText('Notch_1').at(-1)?.closest('li') as HTMLElement;
    expect(within(notch).getByRole('button', { name: 'Remove from the group' })).toHaveFocus();
    // The chat itself gone: the open tab takes the focus.
    view = signedIn({ rooms: [], unread: 0 });
    await act(async () => {
      await emit('friends://state', view);
    });
    expect(within(panel).getByRole('tab', { name: 'Chats' })).toHaveFocus();
    expect(within(panel).getByText('No chats yet. Start one from the Friends tab.')).toBeVisible();
  });

  it('gives the focus to the next friend when one is removed, and back from a report', async () => {
    let view = signedIn({
      friends: [
        { ...ALEX, at: 1, online: true, playing: null },
        { ...NOTCH, at: 2, online: false, playing: null },
      ],
    });
    setup(view, { friends_state: () => view, friends_report: () => null });
    const panel = await openPanel();
    // A report sent or dropped: the focus goes back to the message.
    fireEvent.click(within(panel).getByRole('button', { name: /Alex/ }));
    const report = await within(panel).findByRole('button', { name: 'Report' });
    fireEvent.click(report);
    fireEvent.click(within(panel).getByRole('button', { name: 'Cancel' }));
    expect(within(panel).getByRole('button', { name: 'Report' })).toHaveFocus();
    fireEvent.click(within(panel).getByRole('button', { name: 'Report' }));
    fireEvent.change(within(panel).getByRole('textbox', { name: 'What is wrong with it?' }), {
      target: { value: 'spam' },
    });
    fireEvent.submit(within(panel).getByRole('textbox', { name: 'What is wrong with it?' }));
    await waitFor(() => {
      expect(within(panel).getByText('Reported. Thank you.')).toHaveFocus();
    });
    fireEvent.click(within(panel).getByRole('tab', { name: /Friends/ }));
    const row = within(panel).getByText('Alex').closest('li') as HTMLElement;
    press(within(row).getByRole('button', { name: 'Remove' }));
    press(within(within(row).getByRole('alertdialog')).getByRole('button', { name: 'Remove' }));
    view = signedIn({ friends: [{ ...NOTCH, at: 2, online: false, playing: null }] });
    await act(async () => {
      await emit('friends://state', view);
    });
    const next = within(panel).getByText('Notch_1').closest('li') as HTMLElement;
    expect(within(next).getByRole('button', { name: 'Message' })).toHaveFocus();
  });

  it('keeps "Earlier messages" while they come, then gives the focus to the list', async () => {
    const page = Array.from({ length: 50 }, (_, i) =>
      message(100 + i, ALEX.id, `line ${String(i)}`),
    );
    let finish = () => undefined as unknown;
    let pages = 0;
    setup(signedIn(), {
      friends_messages: () => {
        pages++;
        if (pages === 1) return page;
        return new Promise((done) => {
          finish = () => {
            done([message(1, ALEX.id, 'the first')]);
          };
        });
      },
    });
    const panel = await openPanel();
    fireEvent.click(within(panel).getByRole('button', { name: /Alex/ }));
    const earlier = await within(panel).findByRole('button', { name: 'Earlier messages' });
    earlier.focus();
    fireEvent.click(earlier);
    const waiting = within(panel).getByRole('button', { name: 'Loading messages…' });
    expect(waiting).toBe(earlier);
    expect(waiting).toHaveAttribute('aria-disabled', 'true');
    expect(waiting).toHaveFocus();
    await act(async () => {
      finish();
      await Promise.resolve();
    });
    expect(await within(panel).findByText('the first')).toBeVisible();
    expect(within(panel).getByRole('list', { name: 'Alex' })).toHaveFocus();
  });

  it('shows a chat whose time no date can hold', async () => {
    setup(
      signedIn({ rooms: [direct({ last: { ...message(2, ALEX.id, 'when?'), created: 1e300 } })] }),
    );
    const panel = await openPanel();
    expect(within(panel).getByRole('button', { name: /when\?/ })).toBeVisible();
    expect(panel.querySelector('time')).not.toHaveAttribute('datetime');
  });

  it('stays under a launcher dialog, its scrim and the command palette', async () => {
    setup(signedIn());
    const sheet = () => screen.queryByRole('region', { name: 'Friends and chats' });
    const button = await screen.findByRole('button', { name: /Friends and chats/ });
    fireEvent.click(button);
    // A question of the launcher opens over it: a press on its scrim keeps the sheet.
    const question = render(
      <Dialog title="Start anyway?" onClose={() => undefined}>
        <button type="button">Start</button>
      </Dialog>,
    );
    const dialog = screen.getByRole('dialog', { name: 'Start anyway?' });
    fireEvent.pointerDown(dialog.parentElement as HTMLElement);
    fireEvent.pointerDown(within(dialog).getByRole('button', { name: 'Start' }));
    expect(sheet()).not.toBeNull();
    question.unmount();
    // A modal of its own (the command palette) takes the focus: the sheet waits under it.
    const palette = document.createElement('div');
    palette.setAttribute('aria-modal', 'true');
    const search = document.createElement('input');
    palette.append(search);
    document.body.append(palette);
    try {
      act(() => {
        search.focus();
      });
      fireEvent.keyDown(search, { key: 'Escape' });
      expect(sheet()).not.toBeNull();
    } finally {
      palette.remove();
    }
  });

  it('leaves the focus alone after a press on text, whatever changes in the lists', async () => {
    const request = (person: typeof ALEX, at: number) => ({
      ...person,
      at,
      online: false,
      playing: null,
    });
    const view = signedIn({ incoming: [request(ALEX, 3), request(NOTCH, 4)] });
    setup(view, { friends_state: () => view });
    const panel = await openPanel();
    fireEvent.click(within(panel).getByRole('tab', { name: /Requests/ }));
    const second = within(panel).getByText('Notch_1').closest('li') as HTMLElement;
    within(second).getByRole('button', { name: 'Spam' }).focus();
    // A press on the nick: the focus goes to nothing, and the row stays.
    await act(async () => {
      (document.activeElement as HTMLElement).blur();
      await Promise.resolve();
    });
    await act(async () => {
      await emit('friends://state', signedIn({ incoming: [request(ALEX, 3), request(NOTCH, 4)] }));
    });
    expect(document.body).toHaveFocus();
  });

  it('gives the focus back to the nick field only if it is still where the press left it', async () => {
    let answer: (result: string) => void = () => undefined;
    setup(signedIn(), {
      friends_request: () =>
        new Promise((done) => {
          answer = done;
        }),
    });
    const panel = await openPanel();
    fireEvent.click(within(panel).getByRole('tab', { name: /Friends/ }));
    const field = within(panel).getByRole('textbox', { name: /Add a friend by nick/ });
    fireEvent.change(field, { target: { value: 'Kira_7' } });
    press(within(panel).getByRole('button', { name: 'Send request' }));
    // The player moves on while the request goes.
    const sound = within(panel).getByRole('checkbox', { name: /Sound/ });
    sound.focus();
    await act(async () => {
      answer('sent');
      await Promise.resolve();
    });
    expect(await within(panel).findByText(/Kira_7/)).toBeVisible();
    expect(sound).toHaveFocus();
  });

  it('keeps the first unread message in sight while a card grows under it', async () => {
    const code = 'GrowGrowGrowGrowGrow01';
    let answer: (preview: unknown) => void = () => undefined;
    let page: (messages: ChatMessage[]) => void = () => undefined;
    setup(signedIn(), {
      friends_messages: () =>
        new Promise((done) => {
          page = done;
        }),
      worlds_preview: () =>
        new Promise((done) => {
          answer = done;
        }),
    });
    const offsetTop = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'offsetTop');
    Object.defineProperty(HTMLElement.prototype, 'offsetTop', {
      configurable: true,
      get(this: HTMLElement) {
        return this.getAttribute('role') === 'separator' ? 480 : 0;
      },
    });
    try {
      const panel = await openPanel();
      fireEvent.click(within(panel).getByRole('button', { name: /Alex/ }));
      const list = await within(panel).findByRole('list', { name: 'Alex' });
      // A list 300 px high with 800 px of messages; what the code sets is kept.
      const set: number[] = [];
      let top = 0;
      Object.defineProperties(list, {
        scrollHeight: { configurable: true, get: () => 800 },
        clientHeight: { configurable: true, get: () => 300 },
        scrollTop: {
          configurable: true,
          get: () => top,
          set: (value: number) => {
            top = Math.min(value, 500);
            set.push(value);
          },
        },
      });
      await act(async () => {
        page([message(1, ME.id, 'hi'), message(2, ALEX.id, `🌍 world ${code}`)]);
        await Promise.resolve();
      });
      expect(await within(panel).findByText('Alex invites you to a world')).toBeVisible();
      expect(set.length).toBeGreaterThan(0);
      expect(set.every((value) => value === 472)).toBe(true);
      const before = set.length;
      await act(async () => {
        answer({
          room: ROOM,
          name: 'Our base',
          ownerNick: 'Alex',
          members: 2,
          minecraft: '1.21.1',
          mods: null,
          hosting: false,
        });
        await Promise.resolve();
      });
      expect(await within(panel).findByText('Our base, Minecraft 1.21.1')).toBeVisible();
      await waitFor(() => {
        expect(set.length).toBeGreaterThan(before);
      });
      // The facts came in under it: the first unread message stays where it was put.
      expect(set.every((value) => value === 472)).toBe(true);
    } finally {
      if (offsetTop !== undefined)
        Object.defineProperty(HTMLElement.prototype, 'offsetTop', offsetTop);
    }
  });
});
