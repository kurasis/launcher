import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it } from 'vitest';
import type { AccountSummary, AppSnapshot, MicrosoftAccount } from '../../lib/contracts';
import { snapshotFixture } from '../../test/fixtures';
import { AccountView } from './AccountView';

function setup(account: Partial<AccountSummary>, overrides: Partial<AppSnapshot> = {}) {
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
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
    fireEvent.click(screen.getByRole('button', { name: 'Get Minecraft' }));
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
    expect(screen.queryByRole('button', { name: 'Get Minecraft' })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Check again' }));
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
});
