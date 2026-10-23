import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it, vi } from 'vitest';
import type { LocalWorld, WorldsRoom, WorldsState } from '../../lib/contracts';
import { accountFixture, snapshotFixture } from '../../test/fixtures';
import { WorldsView } from './WorldsView';

const ROOM = '6f1d3b8e-2c4a-4f7e-9b1a-0c2d3e4f5a6b';
const OTHER = '7f1d3b8e-2c4a-4f7e-9b1a-0c2d3e4f5a6b';
const WORLD = '8f1d3b8e-2c4a-4f7e-9b1a-0c2d3e4f5a6b';
const ME = '11111111-1111-4111-8111-111111111111';
const FRIEND = '22222222-2222-4222-8222-222222222222';

const mine: WorldsRoom = {
  id: ROOM,
  name: 'Our base',
  description: 'Built together',
  minecraft: '1.21.1',
  revision: 1,
  owner: ME,
  ownerNick: 'Host_Player',
  role: 'owner',
  world: WORLD,
  members: 2,
  hosting: false,
  online: 0,
  maxPlayers: 8,
  created: 1_760_000_000,
  updated: 1_760_000_000,
  pack: null,
  mods: null,
};

const theirs: WorldsRoom = {
  ...mine,
  id: OTHER,
  name: 'Friend world',
  owner: FRIEND,
  ownerNick: 'Guest_One',
  role: 'member',
  world: null,
  hosting: true,
  online: 1,
};

const local: LocalWorld = {
  id: WORLD,
  name: 'Our base',
  minecraft: '1.21.1',
  eulaAccepted: true,
  serverInstalled: false,
  settings: {
    seed: '',
    preset: 'normal',
    difficulty: 'normal',
    gameMode: 'survival',
    hardcore: false,
    pvp: true,
    maxPlayers: 8,
  },
  createdAt: '2026-10-19T00:00:00Z',
  lastRun: null,
  played: false,
  loader: null,
  mods: 0,
  generation: null,
  source: null,
  updatePending: false,
};

const idle: WorldsState = {
  preparing: null,
  hosting: null,
  guest: null,
  failed: null,
  maintenance: false,
};

/** A LauncherError carrying a worlds.lapi.sh problem. */
function problem(name: string): Error {
  return Object.assign(new Error('worlds.lapi.sh refused this.'), {
    code: 'INVALID_INPUT',
    retryable: false,
    diagnosticId: 'd-1',
    safeDetails: { worldsProblem: name },
  });
}

type Answers = Record<string, (args: Record<string, unknown>) => unknown>;

function setup(answers: Answers = {}, signedIn = true) {
  const calls: Call[] = [];
  mockIPC((cmd, args) => {
    const payload = (args ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args: payload });
    const answer = answers[cmd];
    if (answer !== undefined) return answer(payload);
    switch (cmd) {
      case 'worlds_state':
        return idle;
      case 'worlds_rooms':
        return [mine, theirs];
      case 'worlds_local':
        return [local];
      case 'worlds_versions':
        return [{ id: '1.21.1', name: null }];
      case 'worlds_mod_sources':
        return [];
      case 'worlds_room':
        return {
          room: mine,
          members: [
            { account: ME, nick: 'Host_Player', role: 'owner', state: 'active', joined: 1 },
            { account: FRIEND, nick: 'Guest_One', role: 'member', state: 'active', joined: 2 },
          ],
        };
      case 'plugin:event|listen':
        return 1;
      default:
        return null;
    }
  });
  const onOpenAccounts = vi.fn();
  const account = accountFixture(
    signedIn
      ? {
          launcherAccount: {
            id: ME,
            nick: 'Host_Player',
            emailVerified: true,
            signingIn: false,
            needsSignIn: false,
            problem: null,
          },
        }
      : {},
  );
  render(<WorldsView snapshot={snapshotFixture({ account })} onOpenAccounts={onOpenAccounts} />);
  return { calls, onOpenAccounts };
}

type Call = { cmd: string; args: Record<string, unknown> };
const called = (calls: Call[], cmd: string) => calls.filter((c) => c.cmd === cmd);

describe('WorldsView', () => {
  it('asks to sign in to the launcher account first', () => {
    const { onOpenAccounts, calls } = setup({}, false);
    expect(screen.getByText(/Sign in to your launcher account/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Open accounts' }));
    expect(onOpenAccounts).toHaveBeenCalled();
    expect(called(calls, 'worlds_rooms')).toHaveLength(0);
  });

  it('lists rooms and opens the own world for friends', async () => {
    const { calls } = setup();
    const base = (await screen.findByText('Our base')).closest('li');
    expect(base).not.toBeNull();
    const card = within(base as HTMLElement);
    expect(card.getByText('Kept on this computer')).toBeInTheDocument();
    expect(card.getByText('Closed')).toBeInTheDocument();
    fireEvent.click(card.getByRole('button', { name: 'Open for friends' }));
    await waitFor(() => {
      expect(called(calls, 'worlds_host')).toEqual([{ cmd: 'worlds_host', args: { room: ROOM } }]);
    });
    // A friend's open room can be joined.
    const friend = within(screen.getByText('Friend world').closest('li') as HTMLElement);
    expect(friend.getByText('Open: 1 of 8 playing')).toBeInTheDocument();
    fireEvent.click(friend.getByRole('button', { name: 'Join' }));
    await waitFor(() => {
      expect(called(calls, 'worlds_join')).toHaveLength(1);
    });
  });

  it('plays the hosted world once its game is installed', async () => {
    let ready = false;
    const { calls } = setup({
      worlds_state: () => ({
        ...idle,
        hosting: {
          room: ROOM,
          world: WORLD,
          phase: 'ready',
          players: ['Guest_One'],
          ended: null,
          reopening: false,
          clean: null,
        },
      }),
      worlds_instance: () => ({ instanceId: 'inst-1', ready, consent: null }),
      install_instance: () => ({ operationId: 'op-1', instanceId: 'inst-1' }),
    });
    expect(await screen.findByText('Our base is open · Open')).toBeInTheDocument();
    expect(screen.getByText('1 player: Guest_One')).toBeInTheDocument();
    const card = within((await screen.findByText('Our base')).closest('li') as HTMLElement);
    fireEvent.click(card.getByRole('button', { name: 'Play' }));
    expect(await card.findByText(/being installed/)).toBeInTheDocument();
    expect(called(calls, 'install_instance')).toHaveLength(1);
    expect(called(calls, 'worlds_play')).toHaveLength(0);

    ready = true;
    fireEvent.click(card.getByRole('button', { name: 'Play' }));
    await waitFor(() => {
      expect(called(calls, 'worlds_play')).toHaveLength(1);
    });
    const request = called(calls, 'worlds_play')[0]?.args.request as { payload: unknown };
    expect(request.payload).toEqual({ room: ROOM });
    fireEvent.click(card.getByRole('button', { name: 'Close the world' }));
    await waitFor(() => {
      expect(called(calls, 'worlds_stop_hosting')).toHaveLength(1);
    });
  });

  it('installs the mods of a room for a guest only after asking', async () => {
    const sha256 = 'ab'.repeat(32);
    const modded: WorldsRoom = {
      ...theirs,
      pack: { version: '33333333-3333-4333-8333-333333333333', sha256 },
    };
    const { calls } = setup({
      worlds_rooms: () => [mine, modded],
      worlds_state: () => ({
        ...idle,
        guest: {
          room: OTHER,
          connected: true,
          refused: null,
          path: 'direct',
          reconnecting: false,
          gaveUp: false,
        },
      }),
      worlds_instance: (args) =>
        args.accept === sha256
          ? { instanceId: 'inst-2', ready: true, consent: null }
          : {
              instanceId: 'inst-2',
              ready: false,
              consent: {
                sha256,
                mods: 3,
                newMods: 2,
                names: ['fabric-api.jar', 'sodium.jar'],
                bytes: 2 * 1024 * 1024,
              },
            },
    });
    const card = within((await screen.findByText('Friend world')).closest('li') as HTMLElement);
    fireEvent.click(card.getByRole('button', { name: 'Play' }));
    expect(await card.findByText('This room plays 3 mods')).toBeInTheDocument();
    expect(
      card.getByText(/2 mods are new to this computer, 2.0 MiB to download/),
    ).toBeInTheDocument();
    expect(card.getByText(/fabric-api\.jar, sodium\.jar/)).toBeInTheDocument();
    expect(card.getByText(/Mods are programs/)).toBeInTheDocument();
    expect(called(calls, 'worlds_instance')[0]?.args).toEqual({ room: OTHER, accept: null });
    expect(called(calls, 'worlds_play')).toHaveLength(0);

    // Not now: nothing more happens.
    fireEvent.click(card.getByRole('button', { name: 'Not now' }));
    expect(card.queryByText('This room plays 3 mods')).not.toBeInTheDocument();
    expect(called(calls, 'worlds_instance')).toHaveLength(1);

    fireEvent.click(card.getByRole('button', { name: 'Play' }));
    fireEvent.click(await card.findByRole('button', { name: 'Install the mods' }));
    await waitFor(() => {
      expect(called(calls, 'worlds_play')).toHaveLength(1);
    });
    expect(called(calls, 'worlds_instance')[2]?.args).toEqual({ room: OTHER, accept: sha256 });
  });

  it('shows why a start failed and a refused login', async () => {
    setup({
      worlds_state: () => ({
        ...idle,
        failed: 'nick_unfit',
        guest: {
          room: OTHER,
          connected: false,
          refused: 'ROOM_FULL',
          path: 'relay',
          reconnecting: false,
          gaveUp: true,
        },
      }),
    });
    expect(await screen.findByText(/cannot be a Minecraft name/)).toBeInTheDocument();
    expect(screen.getByText('The world is full.')).toBeInTheDocument();
    expect(screen.getByText('The connection to the host was lost.')).toBeInTheDocument();
    // A lost connection says nothing about its path.
    expect(screen.queryByText(/relay\.lapi\.sh, as no direct path/)).toBeNull();
  });

  it('shows the own open world with its live player count, the host included', async () => {
    setup({
      // The service has not seen the session yet.
      worlds_rooms: () => [mine],
      worlds_state: () => ({
        ...idle,
        hosting: {
          room: ROOM,
          world: WORLD,
          phase: 'ready',
          players: ['Host_Player', 'Guest_One'],
          ended: null,
          reopening: false,
          clean: null,
        },
      }),
    });
    expect(await screen.findByText('Open: 2 of 8 playing')).toBeInTheDocument();
    expect(screen.queryByText('Closed')).toBeNull();
  });

  it('joins again after the connection to the host was lost', async () => {
    const { calls } = setup({
      worlds_state: () => ({
        ...idle,
        guest: {
          room: OTHER,
          connected: false,
          refused: null,
          path: null,
          reconnecting: false,
          gaveUp: false,
        },
      }),
    });
    fireEvent.click(await screen.findByRole('button', { name: 'Join again' }));
    await waitFor(() => {
      expect(called(calls, 'worlds_join')[0]?.args).toEqual({ room: OTHER });
    });
    expect(called(calls, 'worlds_disconnect')).toHaveLength(1);
  });

  it('says it connects again while the tunnel comes back', async () => {
    setup({
      worlds_state: () => ({
        ...idle,
        guest: {
          room: OTHER,
          connected: false,
          refused: null,
          path: null,
          reconnecting: true,
          gaveUp: false,
        },
      }),
    });
    expect(await screen.findByText(/Connecting again/)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Join again' })).toBeNull();
  });

  it('says whether the guest plays directly or through the relay', async () => {
    setup({
      worlds_state: () => ({
        ...idle,
        guest: {
          room: OTHER,
          connected: true,
          refused: null,
          path: 'relay',
          reconnecting: false,
          gaveUp: false,
        },
      }),
    });
    expect(await screen.findByText(/through relay\.lapi\.sh/)).toBeInTheDocument();
  });

  it('says the room is opened again after a lapsed lease, with the world still running', async () => {
    setup({
      worlds_state: () => ({
        ...idle,
        hosting: {
          room: ROOM,
          world: WORLD,
          phase: 'ready',
          players: ['Host_Player'],
          ended: 'session_lost',
          reopening: true,
          clean: null,
        },
      }),
    });
    expect(await screen.findByText(/The room is being opened again/)).toBeInTheDocument();
    expect(screen.queryByText(/opened on another computer/)).toBeNull();
    expect(screen.queryByRole('button', { name: 'Close' })).toBeNull();
  });

  it('turns the relay-only mode on and says what the relay sees', async () => {
    const { calls } = setup();
    const box = await screen.findByRole('checkbox', { name: 'Connect through the relay only' });
    expect(box).not.toBeChecked();
    expect(screen.getByText(/sees the addresses of both computers/)).toBeInTheDocument();
    fireEvent.click(box);
    await waitFor(() => {
      expect(called(calls, 'set_app_settings')[0]?.args).toMatchObject({
        request: { payload: { worldsRelayOnly: true } },
      });
    });
  });

  it('joins a room with an invite code after a look', async () => {
    let wrong = true;
    const { calls } = setup({
      worlds_preview: () =>
        wrong
          ? Promise.reject(problem('invite_invalid'))
          : {
              room: OTHER,
              name: 'Friend world',
              ownerNick: 'Guest_One',
              members: 3,
              minecraft: '1.21.1',
              mods: null,
              hosting: false,
            },
      worlds_redeem: () => theirs,
    });
    await screen.findByText('Our base');
    fireEvent.click(screen.getByRole('button', { name: 'Join with a code' }));
    const dialog = within(screen.getByRole('dialog'));
    fireEvent.change(dialog.getByLabelText('Invite code or link'), {
      target: { value: 'not-a-code' },
    });
    fireEvent.click(dialog.getByRole('button', { name: 'Look' }));
    expect(await dialog.findByText(/wrong, used up or expired/)).toBeInTheDocument();
    expect(called(calls, 'worlds_redeem')).toHaveLength(0);

    wrong = false;
    fireEvent.click(dialog.getByRole('button', { name: 'Look' }));
    expect(
      await dialog.findByText('Friend world, by Guest_One · Minecraft 1.21.1 · 3 members'),
    ).toBeInTheDocument();
    fireEvent.click(dialog.getByRole('button', { name: 'Join the room' }));
    await waitFor(() => {
      expect(called(calls, 'worlds_redeem')).toHaveLength(1);
    });
  });

  it('creates a world only with the EULA accepted', async () => {
    const { calls } = setup({ worlds_create: () => mine, worlds_accept_eula: () => local });
    await screen.findByText('Our base');
    fireEvent.click(screen.getByRole('button', { name: 'Create a world' }));
    const dialog = within(screen.getByRole('dialog'));
    fireEvent.change(dialog.getByLabelText('World name'), { target: { value: ' New world ' } });
    const create = dialog.getByRole('button', { name: 'Create' });
    expect(create).toBeDisabled();
    fireEvent.click(dialog.getByLabelText('I accept the Minecraft EULA'));
    await waitFor(() => {
      expect(create).toBeEnabled();
    });
    fireEvent.click(create);
    await waitFor(() => {
      expect(called(calls, 'worlds_accept_eula')).toEqual([
        { cmd: 'worlds_accept_eula', args: { world: WORLD } },
      ]);
    });
    const payload = called(calls, 'worlds_create')[0]?.args.payload as { name: string };
    expect(payload.name).toBe('New world');
  });

  it('makes a world with the mods of an installation, on its version', async () => {
    const { calls } = setup({
      worlds_versions: () => [
        { id: '26.3', name: null },
        { id: '1.21.1', name: null },
      ],
      worlds_mod_sources: () => [
        {
          instanceId: 'b8f5d6a2-1c3e-4f5a-9b7c-2d4e6f8a0b1c',
          name: 'Fabric pack',
          minecraft: '1.21.1',
          loader: { kind: 'fabric', version: '0.17.2' },
        },
      ],
      worlds_create: () => mine,
      worlds_accept_eula: () => local,
    });
    await screen.findByText('Our base');
    fireEvent.click(screen.getByRole('button', { name: 'Create a world' }));
    const dialog = within(screen.getByRole('dialog'));
    await dialog.findByText(/choose an installation with Fabric/);
    const option = await dialog.findByRole('option', {
      name: 'Fabric pack · Minecraft 1.21.1 · Fabric 0.17.2',
    });
    fireEvent.change(dialog.getByLabelText('Mods'), {
      target: { value: (option as HTMLOptionElement).value },
    });
    await dialog.findByText(/stay out of it/);
    expect(dialog.getByLabelText('Minecraft version')).toHaveValue('1.21.1');
    expect(dialog.getByLabelText('Minecraft version')).toBeDisabled();
    expect(dialog.queryByLabelText('Show snapshots')).toBeNull();
    fireEvent.change(dialog.getByLabelText('World name'), { target: { value: 'Modded' } });
    fireEvent.click(dialog.getByLabelText('I accept the Minecraft EULA'));
    fireEvent.click(dialog.getByRole('button', { name: 'Create' }));
    await waitFor(() => {
      expect(called(calls, 'worlds_create')).toHaveLength(1);
    });
    const payload = called(calls, 'worlds_create')[0]?.args.payload as {
      minecraft: string;
      instanceId: string | null;
    };
    expect(payload.minecraft).toBe('1.21.1');
    expect(payload.instanceId).toBe('b8f5d6a2-1c3e-4f5a-9b7c-2d4e6f8a0b1c');
  });

  it('lists snapshots only when asked, with what they mean', async () => {
    const { calls } = setup({
      worlds_versions: (args) =>
        (args.snapshots === true ? ['26.1-snapshot-2', '1.21.1'] : ['1.21.1']).map((id) => ({
          id,
          name: null,
        })),
      worlds_create: () => mine,
      worlds_accept_eula: () => local,
    });
    await screen.findByText('Our base');
    fireEvent.click(screen.getByRole('button', { name: 'Create a world' }));
    const dialog = within(screen.getByRole('dialog'));
    await waitFor(() => {
      expect(dialog.getByLabelText('Minecraft version')).toHaveValue('1.21.1');
    });
    expect(dialog.queryByText(/Snapshots are test versions/)).toBeNull();
    fireEvent.click(dialog.getByLabelText('Show snapshots'));
    await dialog.findByText(/Snapshots are test versions/);
    await dialog.findByRole('option', { name: '26.1-snapshot-2' });
    expect(called(calls, 'worlds_versions').map((c) => c.args.snapshots)).toEqual([false, true]);
    fireEvent.change(dialog.getByLabelText('Minecraft version'), {
      target: { value: '26.1-snapshot-2' },
    });
    fireEvent.change(dialog.getByLabelText('World name'), { target: { value: 'Test' } });
    fireEvent.click(dialog.getByLabelText('I accept the Minecraft EULA'));
    fireEvent.click(dialog.getByRole('button', { name: 'Create' }));
    await waitFor(() => {
      expect(called(calls, 'worlds_create')).toHaveLength(1);
    });
    const payload = called(calls, 'worlds_create')[0]?.args.payload as { minecraft: string };
    expect(payload.minecraft).toBe('26.1-snapshot-2');
  });

  it('lists the special versions with a server only when asked, by name', async () => {
    const { calls } = setup({
      worlds_versions: (args) => [
        { id: '1.21.1', name: null },
        ...(args.special === true
          ? [
              { id: '24w14potato', name: '24w14potato' },
              { id: 'combat-8c', name: 'Combat Test 8c' },
            ]
          : []),
      ],
      worlds_create: () => mine,
      worlds_accept_eula: () => local,
    });
    await screen.findByText('Our base');
    fireEvent.click(screen.getByRole('button', { name: 'Create a world' }));
    const dialog = within(screen.getByRole('dialog'));
    await waitFor(() => {
      expect(dialog.getByLabelText('Minecraft version')).toHaveValue('1.21.1');
    });
    expect(dialog.queryByRole('group', { name: 'Special' })).toBeNull();
    expect(dialog.queryByText(/not in Mojang's list/)).toBeNull();
    fireEvent.click(dialog.getByLabelText('Show special versions'));
    await dialog.findByText(/not in Mojang's list/);
    const group = within(await dialog.findByRole('group', { name: 'Special' }));
    expect(group.getAllByRole('option').map((o) => o.textContent)).toEqual([
      '24w14potato',
      'Combat Test 8c',
    ]);
    expect(called(calls, 'worlds_versions').map((c) => c.args)).toEqual([
      { snapshots: false, special: false },
      { snapshots: false, special: true },
    ]);
    // The version stays the one chosen.
    expect(dialog.getByLabelText('Minecraft version')).toHaveValue('1.21.1');
    fireEvent.change(dialog.getByLabelText('Minecraft version'), {
      target: { value: 'combat-8c' },
    });
    fireEvent.change(dialog.getByLabelText('World name'), { target: { value: 'Combat' } });
    fireEvent.click(dialog.getByLabelText('I accept the Minecraft EULA'));
    fireEvent.click(dialog.getByRole('button', { name: 'Create' }));
    await waitFor(() => {
      expect(called(calls, 'worlds_create')).toHaveLength(1);
    });
    const payload = called(calls, 'worlds_create')[0]?.args.payload as { minecraft: string };
    expect(payload.minecraft).toBe('combat-8c');
  });

  it('manages members and makes invite codes on a room page', async () => {
    const { calls } = setup({
      worlds_invite: () => ({
        invite: { id: 'i', created: 1, expires: 1_760_086_400, maxUses: 1, uses: 0 },
        code: 'abcdefghijklmnopqrstuv',
      }),
    });
    const card = within((await screen.findByText('Our base')).closest('li') as HTMLElement);
    fireEvent.click(card.getByRole('button', { name: 'Open' }));
    expect(await screen.findByText('Guest_One')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Make a code' }));
    expect(await screen.findByText('abcdefghijklmnopqrstuv')).toBeInTheDocument();
    expect(called(calls, 'worlds_invite')[0]?.args).toEqual({ room: ROOM, hours: 24, uses: 1 });

    // Removing asks first.
    fireEvent.click(screen.getByRole('button', { name: 'Remove' }));
    const dialog = within(screen.getByRole('dialog'));
    expect(dialog.getByText(/Remove Guest_One from the room/)).toBeInTheDocument();
    fireEvent.click(dialog.getByRole('button', { name: 'Remove' }));
    await waitFor(() => {
      expect(called(calls, 'worlds_moderate')[0]?.args).toEqual({
        room: ROOM,
        account: FRIEND,
        action: 'remove',
      });
    });
    // The owner is never offered to be removed, and cannot leave.
    expect(screen.getAllByRole('button', { name: 'Remove' })).toHaveLength(1);
    expect(screen.queryByRole('button', { name: 'Leave the room' })).toBeNull();
  });

  it('sends a fresh invite code to a friend in a direct chat', async () => {
    const { calls } = setup({
      worlds_invite: () => ({
        invite: { id: 'i', created: 1, expires: 1_760_086_400, maxUses: 1, uses: 0 },
        code: 'abcdefghijklmnopqrstuv',
      }),
      friends_state: () => ({
        signedIn: true,
        connected: true,
        me: { id: ME, nick: 'Host_Player' },
        friends: [{ id: FRIEND, nick: 'Guest_One', at: 1, online: true, playing: null }],
        incoming: [],
        outgoing: [],
        blocked: [],
        rooms: [],
        unread: 0,
        visible: true,
        sanction: null,
        problem: null,
      }),
      friends_open_direct: () => 'chat-1',
      friends_send: () => ({ id: 1, room: 'chat-1', sender: ME, body: '', created: 1 }),
    });
    const card = within((await screen.findByText('Our base')).closest('li') as HTMLElement);
    fireEvent.click(card.getByRole('button', { name: 'Open' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Make a code' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Send in a chat' }));
    expect(await screen.findByText('Code sent to Guest_One.')).toBeInTheDocument();
    expect(called(calls, 'friends_open_direct')[0]?.args).toEqual({ id: FRIEND });
    expect(called(calls, 'friends_send')[0]?.args).toEqual({
      room: 'chat-1',
      body: '🌍 world abcdefghijklmnopqrstuv',
    });
  });

  it('lists the computers and signs another one out after asking', async () => {
    let devices = [
      { id: 'd-1', name: 'Windows x86_64', created: 1_760_000_000, current: true },
      { id: 'd-2', name: 'Linux aarch64', created: 1_760_000_000, current: false },
    ];
    const { calls } = setup({
      worlds_devices: () => devices,
      worlds_revoke_device: () => {
        devices = devices.filter((d) => d.id !== 'd-2');
        return null;
      },
    });
    fireEvent.click(await screen.findByRole('button', { name: 'Computers' }));
    const dialog = within(await screen.findByRole('dialog', { name: 'Computers with worlds' }));
    expect(await dialog.findByText('This computer')).toBeInTheDocument();
    // This computer cannot be signed out from here; another one can.
    expect(dialog.getAllByRole('button', { name: 'Sign out' })).toHaveLength(1);
    fireEvent.click(dialog.getByRole('button', { name: 'Sign out' }));
    expect(dialog.getByText('Sign out Linux aarch64?')).toBeInTheDocument();
    fireEvent.click(dialog.getByRole('button', { name: 'Sign out' }));
    await waitFor(() => {
      expect(dialog.queryByText('Linux aarch64')).toBeNull();
    });
    expect(called(calls, 'worlds_revoke_device')[0]?.args).toEqual({ device: 'd-2' });
  });

  it('imports a single-player world as a copy, with the EULA accepted', async () => {
    const INSTANCE = '33333333-3333-4333-8333-333333333333';
    const { calls } = setup({
      worlds_importable: () => [
        {
          instanceId: INSTANCE,
          instanceName: 'Vanilla',
          minecraft: '1.21.1',
          loader: null,
          saves: ['Home', 'Far'],
        },
      ],
      worlds_import: () => mine,
      worlds_accept_eula: () => local,
    });
    await screen.findByText('Our base');
    fireEvent.click(screen.getByRole('button', { name: 'Import a world' }));
    const dialog = within(await screen.findByRole('dialog'));
    const name = await dialog.findByLabelText('World name');
    expect(name).toHaveValue('Home');
    // The name follows the chosen world.
    fireEvent.change(dialog.getByLabelText('World'), { target: { value: 'Far' } });
    expect(name).toHaveValue('Far');
    const submit = dialog.getByRole('button', { name: 'Import' });
    expect(submit).toBeDisabled();
    fireEvent.click(dialog.getByLabelText('I accept the Minecraft EULA'));
    fireEvent.click(submit);
    await waitFor(() => {
      expect(called(calls, 'worlds_accept_eula')).toHaveLength(1);
    });
    expect(called(calls, 'worlds_import')[0]?.args).toEqual({
      payload: { instanceId: INSTANCE, save: 'Far', name: 'Far', description: '' },
    });
  });

  it('says when nothing can be imported', async () => {
    setup({ worlds_importable: () => [] });
    await screen.findByText('Our base');
    fireEvent.click(screen.getByRole('button', { name: 'Import a world' }));
    expect(await screen.findByText(/No installation has single-player worlds/)).toBeInTheDocument();
  });

  it('restores a backup after naming its date, and exports the world', async () => {
    const backup = {
      id: '44444444-4444-4444-8444-444444444444',
      instanceId: WORLD,
      instanceName: 'Our base',
      versionLabel: 'Minecraft 1.21.1',
      versionId: '1.21.1',
      scope: { kind: 'world', name: 'world' },
      reason: 'after_play',
      createdAt: '2026-10-19T12:00:00Z',
      fileCount: 12,
      totalBytes: 2048,
      newBytes: 1024,
    };
    const { calls } = setup({
      worlds_local: () => [{ ...local, played: true }],
      worlds_backups: () => [backup],
      worlds_export: () => ({ saved: true, fileName: 'Our base.zip' }),
    });
    const card = within((await screen.findByText('Our base')).closest('li') as HTMLElement);
    fireEvent.click(card.getByRole('button', { name: 'Open' }));
    const list = within(await screen.findByRole('list', { name: 'Backups of this world' }));
    expect(list.getByText(/After playing/)).toBeInTheDocument();
    fireEvent.click(list.getByRole('button', { name: 'Restore' }));
    const dialog = within(screen.getByRole('dialog'));
    const date = new Date(backup.createdAt).toLocaleString('en');
    expect(
      dialog.getByText((text) => text.includes(`from ${date} in place of Our base`)),
    ).toBeInTheDocument();
    fireEvent.click(dialog.getByRole('button', { name: 'Restore' }));
    await waitFor(() => {
      expect(called(calls, 'worlds_restore')[0]?.args).toEqual({ world: WORLD, backup: backup.id });
    });
    fireEvent.click(screen.getByRole('button', { name: 'Export as ZIP' }));
    expect(await screen.findByText('Saved as Our base.zip.')).toBeInTheDocument();
    expect(called(calls, 'worlds_export')[0]?.args).toEqual({
      world: WORLD,
      title: 'Export the world',
    });
  });

  it('updates the mods of a world after showing what changes and asking for changed settings', async () => {
    const source = 'b8f5d6a2-1c3e-4f5a-9b7c-2d4e6f8a0b1c';
    const fabric = { kind: 'fabric', version: '0.17.2' };
    const { calls } = setup({
      worlds_local: () => [
        { ...local, played: true, loader: fabric, mods: 2, generation: 1, source },
      ],
      worlds_backups: () => [],
      worlds_mod_sources: () => [
        { instanceId: source, name: 'Fabric pack', minecraft: '1.21.1', loader: fabric },
        {
          instanceId: 'c8f5d6a2-1c3e-4f5a-9b7c-2d4e6f8a0b1c',
          name: 'Older pack',
          minecraft: '1.20.1',
          loader: fabric,
        },
      ],
      worlds_update_preview: () => ({
        fingerprint: 'f'.repeat(64),
        generation: 1,
        next: 2,
        loader: fabric,
        loaderChanged: false,
        added: [{ name: 'sodium.jar', side: 'client' }],
        removed: [{ name: 'ledger.jar', side: 'server' }],
        updated: [{ name: 'fabric-api-2.jar', side: 'both', from: 'fabric-api-1.jar' }],
        settings: 1,
        conflicts: [
          {
            path: 'config/spawn.json',
            base: { text: '{"spawn":1}', bytes: 11, truncated: false },
            mine: { text: '{"spawn":7}', bytes: 11, truncated: false },
            theirs: { text: '{"spawn":2}', bytes: 11, truncated: false },
          },
        ],
        same: false,
      }),
    });
    const card = within((await screen.findByText('Our base')).closest('li') as HTMLElement);
    fireEvent.click(card.getByRole('button', { name: 'Open' }));
    expect(await screen.findByText("Set 1 of the world's mods, 2 mods.")).toBeInTheDocument();
    const from = screen.getByLabelText('From the installation');
    await within(from).findByText('Fabric pack');
    expect(within(from).queryByText('Older pack')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Show the changes' }));
    expect(await screen.findByText('sodium.jar')).toBeInTheDocument();
    expect(screen.getByText('fabric-api-2.jar, was fabric-api-1.jar')).toBeInTheDocument();
    expect(screen.getByText(/1 setting you have not changed takes/)).toBeInTheDocument();
    expect(screen.getByText('{"spawn":7}')).toBeInTheDocument();
    expect(called(calls, 'worlds_update_preview')[0]?.args).toEqual({
      world: WORLD,
      instanceId: source,
    });
    const update = screen.getByRole('button', { name: 'Update the world' });
    expect(update).toBeDisabled();
    fireEvent.click(screen.getByLabelText('Take the new one'));
    fireEvent.click(update);
    expect(await screen.findByText(/The world has set 2 of its mods/)).toBeInTheDocument();
    expect(called(calls, 'worlds_update')[0]?.args.payload).toEqual({
      room: ROOM,
      world: WORLD,
      instanceId: source,
      fingerprint: 'f'.repeat(64),
      choices: { 'config/spawn.json': 'theirs' },
    });
    expect(called(calls, 'worlds_stop_hosting')).toHaveLength(0);
  });

  it('says the new mods did not start on the copy and the world stayed as it was', async () => {
    const fabric = { kind: 'fabric', version: '0.17.2' };
    setup({
      worlds_local: () => [{ ...local, played: true, loader: fabric, mods: 1, generation: 1 }],
      worlds_backups: () => [],
      worlds_mod_sources: () => [
        {
          instanceId: 'b8f5d6a2-1c3e-4f5a-9b7c-2d4e6f8a0b1c',
          name: 'Pack',
          minecraft: '1.21.1',
          loader: fabric,
        },
      ],
      worlds_update_preview: () => ({
        fingerprint: 'a'.repeat(64),
        generation: 1,
        next: 2,
        loader: fabric,
        loaderChanged: false,
        added: [{ name: 'broken.jar', side: 'both' }],
        removed: [],
        updated: [],
        settings: 0,
        conflicts: [],
        same: false,
      }),
      worlds_update: () => {
        throw Object.assign(new Error('The world server stopped while starting.'), {
          code: 'GAME_EXITED_WITH_ERROR',
          retryable: false,
          diagnosticId: 'd-2',
          safeDetails: { worldsTrialFailed: true },
        });
      },
    });
    const card = within((await screen.findByText('Our base')).closest('li') as HTMLElement);
    fireEvent.click(card.getByRole('button', { name: 'Open' }));
    await screen.findByRole('option', { name: 'Pack' });
    fireEvent.click(screen.getByRole('button', { name: 'Show the changes' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Update the world' }));
    expect(
      await screen.findByText(/did not start on a copy of the world, so the world has not changed/),
    ).toBeInTheDocument();
  });

  it('waits with backups while the world is open', async () => {
    setup({
      worlds_state: () => ({
        ...idle,
        hosting: {
          room: ROOM,
          world: WORLD,
          phase: 'ready',
          players: [],
          ended: null,
          reopening: false,
          clean: null,
        },
      }),
      worlds_local: () => [{ ...local, played: true }],
      worlds_backups: () => [],
    });
    const card = within((await screen.findByText('Our base')).closest('li') as HTMLElement);
    fireEvent.click(card.getByRole('button', { name: 'Open' }));
    expect(await screen.findByText(/Close the world to back it up/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Back up now' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Export as ZIP' })).toBeDisabled();
  });
});
