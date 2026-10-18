import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { emit } from '@tauri-apps/api/event';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it, vi } from 'vitest';
import type { ChatMessage, ChatRoom, FriendsView } from '../../lib/contracts';
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
      { ...ALEX, at: 1 },
      { ...NOTCH, at: 2 },
    ],
    incoming: [],
    outgoing: [],
    blocked: [],
    rooms: [direct()],
    unread: 1,
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
  problem: null,
};

/** Answers friends commands; a function answer may throw a problem string. */
function setup(view: FriendsView, answers: Record<string, (args: unknown) => unknown> = {}) {
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
  render(<FriendsCorner onOpenAccounts={onOpenAccounts} />);
  return { calls, onOpenAccounts };
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
    setup(signedIn({ incoming: [{ ...NOTCH, at: 3 }] }));
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
    setup(signedIn({ friends: [{ ...NOTCH, at: 2 }] }));
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
        incoming: [{ ...NOTCH, at: 3 }],
        outgoing: [{ id: 'x', nick: 'Herobrine', at: 4 }],
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
});
