import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it, vi } from 'vitest';
import type { AccountSummary, AppSnapshot, MicrosoftAccount } from '../../lib/contracts';
import { snapshotFixture } from '../../test/fixtures';
import { AccountView } from './AccountView';

const HEAD = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAgAAAAI';

function setup(
  account: Partial<AccountSummary>,
  overrides: Partial<AppSnapshot> = {},
  heads: unknown = {},
) {
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === 'skins_microsoft_heads') return heads;
    if (cmd === 'begin_login') return { operationId: 'op-1' };
    if (cmd === 'add_notoken_profile') return { id: 'p-1', name: 'Alex' };
    if (cmd === 'get_app_snapshot') return snapshotFixture();
    return null;
  });
  const base = snapshotFixture({ microsoftClientId: 'configured_unverified', ...overrides });
  render(<AccountView snapshot={{ ...base, account: { ...base.account, ...account } }} />);
  return calls;
}

const named = (cmd: string, calls: { cmd: string; args: unknown }[]) =>
  calls.filter((c) => c.cmd === cmd);

/** The list row of an account or profile, by its name. */
function row(name: string): HTMLElement {
  const item = screen.getByText(name).closest('li');
  if (item === null) throw new Error(`no row for ${name}`);
  return item;
}

function microsoft(overrides: Partial<MicrosoftAccount> = {}): MicrosoftAccount {
  return {
    profileId: 'steve',
    profileName: 'Steve_Test',
    state: 'verified_access',
    error: null,
    ...overrides,
  };
}

describe('AccountView', () => {
  it('explains a missing client ID instead of offering sign-in', () => {
    setup({}, { microsoftClientId: 'not_configured' });
    expect(screen.getByText(/no project-owned Microsoft application ID/)).toBeVisible();
    expect(screen.queryByRole('button', { name: /Add a Microsoft account/ })).toBeNull();
    expect(screen.queryByLabelText(/password/i)).not.toBeInTheDocument();
  });

  it('adds a Microsoft account through browser sign-in', async () => {
    const calls = setup({});
    expect(screen.getByText('No Microsoft account added yet.')).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Add a Microsoft account' }));
    await waitFor(() => {
      expect(named('begin_login', calls)).toHaveLength(1);
    });
    expect(screen.getByText(/never sees your password/)).toBeVisible();
  });

  it('offers cancel while waiting for the browser', async () => {
    const calls = setup({ loginOperationId: 'op-7', loginState: 'signing_in' });
    expect(screen.getByRole('status')).toHaveTextContent('Finish signing in');
    fireEvent.click(screen.getByRole('button', { name: 'Cancel sign-in' }));
    await waitFor(() => {
      expect(named('cancel_login', calls)[0]?.args).toMatchObject({
        request: { payload: { operationId: 'op-7' } },
      });
    });
  });

  it('lists several accounts, chooses the default and removes one', async () => {
    const calls = setup({
      accounts: [microsoft(), microsoft({ profileId: 'alex', profileName: 'Alex_Test' })],
      defaultProfile: { kind: 'microsoft', id: 'steve' },
    });
    expect(screen.getByText('Steve_Test')).toBeVisible();
    expect(screen.getByText('Alex_Test')).toBeVisible();
    expect(screen.getAllByText('Default')).toHaveLength(1);
    fireEvent.click(screen.getByRole('button', { name: 'Use by default' }));
    await waitFor(() => {
      expect(named('set_default_profile', calls)[0]?.args).toMatchObject({
        request: { payload: { profile: { kind: 'microsoft', id: 'alex' } } },
      });
    });
    // Removing asks first; Cancel removes nothing.
    fireEvent.click(screen.getAllByRole('button', { name: 'Remove' })[0] as HTMLElement);
    expect(screen.getByRole('alertdialog')).toHaveTextContent(
      'Remove Steve_Test from the launcher?',
    );
    fireEvent.click(
      within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Cancel' }),
    );
    expect(named('sign_out', calls)).toHaveLength(0);
    fireEvent.click(screen.getAllByRole('button', { name: 'Remove' })[0] as HTMLElement);
    fireEvent.click(
      within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Remove' }),
    );
    await waitFor(() => {
      expect(named('sign_out', calls)[0]?.args).toMatchObject({
        request: { payload: { profileId: 'steve' } },
      });
    });
    // Another account can still be added.
    expect(screen.getByRole('button', { name: 'Add a Microsoft account' })).toBeVisible();
  });

  it('keeps no-access distinct from service outage', async () => {
    const calls = setup({
      accounts: [
        microsoft({
          state: 'no_access',
          error: {
            code: 'NO_GAME_ACCESS',
            message: 'This Microsoft account does not currently have access.',
            retryable: false,
            diagnosticId: 'd',
          },
        }),
      ],
    });
    expect(screen.getByText('No Java Edition access')).toBeVisible();
    // The account's own row says what helps; the panel's foot links to every page.
    fireEvent.click(within(row('Steve_Test')).getByRole('button', { name: 'Get Minecraft' }));
    await waitFor(() => {
      expect(named('open_external', calls)[0]?.args).toEqual({ link: 'buy_minecraft' });
    });
  });

  it('service outage offers a re-check of that account, not a purchase', async () => {
    const calls = setup({
      accounts: [
        microsoft({
          state: 'service_unavailable',
          error: {
            code: 'SERVICE_UNAVAILABLE',
            message: 'temporarily unavailable',
            retryable: true,
            diagnosticId: 'd',
          },
        }),
      ],
    });
    expect(
      within(row('Steve_Test')).queryByRole('button', { name: 'Get Minecraft' }),
    ).not.toBeInTheDocument();
    fireEvent.click(within(row('Steve_Test')).getByRole('button', { name: 'Check again' }));
    await waitFor(() => {
      expect(named('verify_account', calls)[0]?.args).toMatchObject({
        request: { payload: { profileId: 'steve' } },
      });
    });
  });

  it('reports incomplete sign-out and session-only mode', async () => {
    const calls = setup({
      pendingSignOuts: [{ profileId: 'gone', profileName: 'Old_Name' }],
      sessionOnly: true,
    });
    expect(screen.getByText(/Old_Name: the saved sign-in could not be removed/)).toBeVisible();
    expect(screen.getByText(/credential store is unavailable/)).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Try removing again' }));
    await waitFor(() => {
      expect(named('sign_out', calls)[0]?.args).toMatchObject({
        request: { payload: { profileId: 'gone' } },
      });
    });
  });

  it('adds and removes no-token profiles for the demo', async () => {
    const calls = setup({ notokenProfiles: [{ id: 'p-0', name: 'Tester' }] });
    expect(screen.getByText('Tester')).toBeVisible();
    expect(screen.getByText(/installations play the demo as Player/)).toBeVisible();
    fireEvent.change(screen.getByLabelText('Player name'), { target: { value: ' Alex ' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add profile' }));
    await waitFor(() => {
      expect(named('add_notoken_profile', calls)[0]?.args).toMatchObject({
        request: { payload: { name: 'Alex' } },
      });
    });
    await waitFor(() => {
      expect(screen.getByLabelText('Player name')).toHaveValue('');
    });
    fireEvent.click(screen.getByRole('button', { name: 'Remove' }));
    expect(named('remove_notoken_profile', calls)).toHaveLength(0);
    fireEvent.click(
      within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Remove' }),
    );
    await waitFor(() => {
      expect(named('remove_notoken_profile', calls)[0]?.args).toMatchObject({
        request: { payload: { profileId: 'p-0' } },
      });
    });
  });

  it('keeps profiles in the demo until someone who owns Minecraft signs in', () => {
    setup({ fullGameAllowed: false });
    const modes = screen.getAllByRole('radiogroup', { name: 'Launch mode' });
    const offline = within(modes[0] as HTMLElement).getByRole('radio', { name: 'Offline' });
    expect(offline).toBeDisabled();
    expect(within(modes[0] as HTMLElement).getByRole('radio', { name: 'Demo' })).toBeChecked();
    expect(screen.getByText(/Offline needs a Microsoft account that owns Minecraft/)).toBeVisible();
  });

  it('switches profiles without an account to offline', async () => {
    const calls = setup({ fullGameAllowed: true, accounts: [microsoft()] });
    const modes = screen.getAllByRole('radiogroup', { name: 'Launch mode' });
    fireEvent.click(within(modes[0] as HTMLElement).getByRole('radio', { name: 'Offline' }));
    await waitFor(() => {
      expect(
        named('set_app_settings', calls).map(
          (c) => (c.args as { request: { payload: unknown } }).request.payload,
        ),
      ).toEqual([{ notokenOffline: true }]);
    });
  });

  it('shows each Microsoft account with the head the core cut from its skin', async () => {
    const calls = setup(
      {
        accounts: [microsoft(), microsoft({ profileId: 'alex', profileName: 'Alex_Test' })],
      },
      {},
      // Steve's is a PNG; Alex's answer is not one and gets the default face.
      { steve: HEAD, alex: 'javascript:alert(1)', stranger: HEAD },
    );
    await waitFor(() => {
      expect(row('Steve_Test').querySelector('img')).toHaveAttribute('src', HEAD);
    });
    expect(row('Alex_Test').querySelector('img')).toBeNull();
    expect(row('Alex_Test').querySelector('svg[viewBox="0 0 8 8"]')).not.toBeNull();
    expect(named('skins_microsoft_heads', calls)).toHaveLength(1);
  });

  it('keeps the default faces when the heads cannot be had', async () => {
    const calls = setup({ accounts: [microsoft()] }, {}, null);
    await waitFor(() => {
      expect(named('skins_microsoft_heads', calls)).toHaveLength(1);
    });
    expect(row('Steve_Test').querySelector('img')).toBeNull();
  });

  it('asks for no heads without Microsoft accounts', () => {
    const calls = setup({});
    expect(named('skins_microsoft_heads', calls)).toHaveLength(0);
  });

  it('checks a signed-in account again and asks one that ended to sign in first', async () => {
    const calls = setup({
      accounts: [
        microsoft(),
        microsoft({ profileId: 'alex', profileName: 'Alex_Test', state: 'reauth_required' }),
      ],
      defaultProfile: { kind: 'microsoft', id: 'steve' },
    });
    expect(within(row('Steve_Test')).getByText(/checked again every time/)).toBeVisible();
    fireEvent.click(within(row('Steve_Test')).getByRole('button', { name: 'Check again' }));
    await waitFor(() => {
      expect(named('verify_account', calls)[0]?.args).toMatchObject({
        request: { payload: { profileId: 'steve' } },
      });
    });
    const ended = within(row('Alex_Test'));
    expect(ended.queryByRole('button', { name: 'Check again' })).toBeNull();
    expect(ended.getAllByRole('button')[0]).toHaveTextContent('Sign in again');
    expect(ended.getByText(/never sees your password/)).toBeVisible();
    fireEvent.click(ended.getByRole('button', { name: 'Sign in again' }));
    await waitFor(() => {
      expect(named('begin_login', calls)).toHaveLength(1);
    });
  });

  it('links to every Microsoft and Minecraft page at the foot, opened by the core', async () => {
    const calls = setup({}, { microsoftClientId: 'not_configured' });
    const links = screen.getByRole('list', { name: 'Microsoft and Minecraft pages' });
    const names = within(links)
      .getAllByRole('button')
      .map((button) => button.textContent);
    expect(names).toEqual([
      'Get Minecraft',
      'Create your Java profile',
      'Open Xbox account',
      'Microsoft family settings',
      'Minecraft help',
      'Minecraft EULA',
    ]);
    fireEvent.click(within(links).getByRole('button', { name: 'Minecraft EULA' }));
    await waitFor(() => {
      expect(named('open_external', calls)[0]?.args).toEqual({ link: 'minecraft_eula' });
    });
  });

  it('sends the skin and cape to the Skins tab instead of showing them here', () => {
    mockIPC(() => null);
    const base = snapshotFixture();
    const onOpenSkins = vi.fn();
    render(
      <AccountView
        snapshot={{
          ...base,
          account: {
            ...base.account,
            launcherAccount: {
              id: 'acc-1',
              nick: 'Steve_2',
              emailVerified: true,
              signingIn: false,
              needsSignIn: false,
              problem: null,
            },
          },
        }}
        onOpenSkins={onOpenSkins}
      />,
    );
    expect(screen.queryByRole('region', { name: 'Skin and cape' })).toBeNull();
    expect(screen.getByText(/changed in Skins/)).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Open Skins' }));
    expect(onOpenSkins).toHaveBeenCalledTimes(1);
  });

  it('says what offline means for profiles only when it is chosen or closed', () => {
    setup({ fullGameAllowed: true, notokenProfiles: [{ id: 'p-0', name: 'Tester' }] });
    const panel = screen.getByRole('region', { name: 'Profiles without an account' });
    expect(within(panel).queryByText(/full game without the account check/)).toBeNull();
    expect(within(row('Tester')).getByText('Demo')).toBeVisible();
  });

  it('shows offline profiles as offline', () => {
    setup(
      { fullGameAllowed: true, notokenProfiles: [{ id: 'p-0', name: 'Tester' }] },
      {
        settings: { ...snapshotFixture().settings, notokenOffline: true },
      },
    );
    const panel = screen.getByRole('region', { name: 'Profiles without an account' });
    expect(within(panel).getByText(/full game without the account check/)).toBeVisible();
    expect(within(row('Tester')).getByText('Offline')).toBeVisible();
  });

  it('moves between demo and offline with the arrow keys, one tab stop for both', async () => {
    const calls = setup({ fullGameAllowed: true });
    const panel = screen.getByRole('region', { name: 'Profiles without an account' });
    const demo = within(panel).getByRole('radio', { name: 'Demo' });
    const offline = within(panel).getByRole('radio', { name: 'Offline' });
    expect(demo).toHaveAttribute('tabindex', '0');
    expect(offline).toHaveAttribute('tabindex', '-1');
    fireEvent.keyDown(demo, { key: 'ArrowRight' });
    await waitFor(() => {
      expect(
        named('set_app_settings', calls).map(
          (c) => (c.args as { request: { payload: unknown } }).request.payload,
        ),
      ).toEqual([{ notokenOffline: true }]);
    });
    expect(offline).toHaveFocus();
  });

  it('keeps the arrow keys on demo while offline is closed', () => {
    const calls = setup({ fullGameAllowed: false });
    const panel = screen.getByRole('region', { name: 'Profiles without an account' });
    fireEvent.keyDown(within(panel).getByRole('radio', { name: 'Demo' }), { key: 'ArrowLeft' });
    expect(named('set_app_settings', calls)).toHaveLength(0);
  });

  it('asks before removing and waits on Cancel', () => {
    setup({ notokenProfiles: [{ id: 'p-0', name: 'Tester' }] });
    fireEvent.click(within(row('Tester')).getByRole('button', { name: 'Remove' }));
    expect(
      within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Cancel' }),
    ).toHaveFocus();
  });
});
