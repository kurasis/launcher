import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it } from 'vitest';
import type { AccountSummary, LauncherAccountView } from '../../lib/contracts';
import { snapshotFixture } from '../../test/fixtures';
import { PlayAs } from '../instances/PlayAs';
import { AccountView } from './AccountView';

function view(overrides: Partial<LauncherAccountView> = {}): LauncherAccountView {
  return {
    id: 'acc-1',
    nick: 'Steve_2',
    emailVerified: false,
    signingIn: false,
    needsSignIn: false,
    problem: null,
    ...overrides,
  };
}

function setup(launcherAccount: LauncherAccountView, answer: unknown = null) {
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === 'get_app_snapshot') return snapshotFixture();
    if (cmd === 'launcher_account_set_nick') return answer;
    if (cmd === 'launcher_account_recheck') return answer;
    if (cmd === 'launcher_account_export') {
      // The core rejects with the problem's name, a plain string.
      // eslint-disable-next-line @typescript-eslint/only-throw-error
      if (answer === 'not_saved') throw 'not_saved';
      return 'launcher-account-Steve_2.json';
    }
    return null;
  });
  const base = snapshotFixture();
  const account: AccountSummary = { ...base.account, launcherAccount };
  render(<AccountView snapshot={{ ...base, account }} />);
  return { calls, account };
}

describe('launcher account', () => {
  it('signs in or registers in its own window, never with a password here', async () => {
    const { calls } = setup(view({ id: null, nick: null }));
    expect(screen.getByRole('heading', { name: 'Launcher account' })).toBeVisible();
    expect(screen.queryByLabelText(/password/i)).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Create account' }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'launcher_account_open')?.args).toEqual({
        screen: 'register',
        title: 'Launcher account: create',
      });
    });
    fireEvent.click(screen.getByRole('button', { name: 'Sign in' }));
    await waitFor(() => {
      expect(calls.filter((c) => c.cmd === 'launcher_account_open')[1]?.args).toEqual({
        screen: 'login',
        title: 'Launcher account: sign in',
      });
    });
  });

  it('shows the account, changes the nick and says why one is refused', async () => {
    const { calls } = setup(view(), 'nick_taken');
    expect(screen.getByText('Steve_2')).toBeVisible();
    expect(screen.getByText('Email not confirmed yet')).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Change nick' }));
    const input = screen.getByRole('textbox', { name: /Nick/ });
    expect(input).toHaveValue('Steve_2');
    fireEvent.change(input, { target: { value: 'Taken' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(await screen.findByText('This nick is taken.')).toBeVisible();
    expect(calls.find((c) => c.cmd === 'launcher_account_set_nick')?.args).toEqual({
      nick: 'Taken',
    });
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    fireEvent.click(screen.getByRole('button', { name: 'Send the letter again' }));
    await waitFor(() => {
      expect(calls.some((c) => c.cmd === 'launcher_account_resend_confirmation')).toBe(true);
    });
    fireEvent.click(screen.getByRole('button', { name: 'Sign out' }));
    await waitFor(() => {
      expect(calls.some((c) => c.cmd === 'launcher_account_sign_out')).toBe(true);
    });
  });

  it('saves my data and deletes the account only on its own page', async () => {
    const { calls } = setup(view());
    fireEvent.click(screen.getByRole('button', { name: 'Download my data' }));
    expect(await screen.findByText('Saved as launcher-account-Steve_2.json.')).toBeVisible();
    expect(calls.find((c) => c.cmd === 'launcher_account_export')?.args).toEqual({
      title: 'Save my launcher account data',
    });
    fireEvent.click(screen.getByRole('button', { name: 'Delete account…' }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'launcher_account_open')?.args).toEqual({
        screen: 'delete',
        title: 'Launcher account: delete',
      });
    });
    expect(screen.queryByLabelText(/password/i)).not.toBeInTheDocument();
  });

  it('says when the export could not be saved', async () => {
    setup(view(), 'not_saved');
    fireEvent.click(screen.getByRole('button', { name: 'Download my data' }));
    expect(
      await screen.findByText('The file could not be saved there. Choose another place.'),
    ).toBeVisible();
  });

  it('asks to sign in again when the saved sign-in ended', () => {
    setup(view({ needsSignIn: true, problem: 'sign_in_again' }));
    expect(screen.getByText('The sign-in has ended. Sign in again.')).toBeVisible();
    expect(screen.getByRole('button', { name: 'Sign in' })).toBeVisible();
  });

  it('a nick is shown as text, whatever it holds', () => {
    setup(view({ nick: '<img src=x onerror=alert(1)>' }));
    expect(screen.getByText('<img src=x onerror=alert(1)>')).toBeVisible();
    expect(document.querySelector('img[src="x"]')).toBeNull();
  });

  it('can be chosen in Play as', async () => {
    const calls: { cmd: string; args: unknown }[] = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === 'get_app_snapshot') return snapshotFixture();
      return null;
    });
    const base = snapshotFixture();
    const instance = {
      ...(base.instances[0] ?? ({} as never)),
      id: '00000000-0000-4000-8000-000000000001',
      profile: null,
    };
    render(<PlayAs instance={instance} account={{ ...base.account, launcherAccount: view() }} />);
    const select = screen.getByRole('combobox', { name: 'Play as' });
    expect(screen.getByRole('option', { name: 'Steve_2 (launcher account)' })).toBeInTheDocument();
    fireEvent.change(select, { target: { value: 'launcher:acc-1' } });
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'set_instance_profile')?.args).toMatchObject({
        request: { payload: { profile: { kind: 'launcher', id: 'acc-1' } } },
      });
    });
  });

  it('checks again whether the email is confirmed, with its own button', async () => {
    const { calls } = setup(view(), true);
    fireEvent.click(screen.getByRole('button', { name: 'Check again' }));
    expect(await screen.findByText(/Email confirmed: friends and chats are open/)).toBeVisible();
    expect(calls.filter((c) => c.cmd === 'launcher_account_recheck')).toHaveLength(1);
  });

  it('says when the email is still not confirmed', async () => {
    setup(view(), false);
    fireEvent.click(screen.getByRole('button', { name: 'Check again' }));
    expect(await screen.findByText(/Not confirmed yet/)).toBeVisible();
  });

  it('has no check button once the email is confirmed', () => {
    setup(view({ emailVerified: true }));
    expect(screen.queryByRole('button', { name: 'Check again' })).toBeNull();
  });

  it('switches the launcher account between the demo and offline', async () => {
    const calls: { cmd: string; args: unknown }[] = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === 'get_app_snapshot') return snapshotFixture();
      return null;
    });
    const base = snapshotFixture();
    render(
      <AccountView
        snapshot={{
          ...base,
          account: { ...base.account, launcherAccount: view(), fullGameAllowed: true },
        }}
      />,
    );
    const panel = screen.getByRole('heading', { name: 'Launcher account' }).closest('section');
    const mode = within(panel as HTMLElement).getByRole('radiogroup', { name: 'Launch mode' });
    fireEvent.click(within(mode).getByRole('radio', { name: 'Offline' }));
    await waitFor(() => {
      expect(
        calls
          .filter((c) => c.cmd === 'set_app_settings')
          .map((c) => (c.args as { request: { payload: unknown } }).request.payload),
      ).toEqual([{ launcherAccountOffline: true }]);
    });
  });

  it('says why offline is closed, or what it is, beside its own switch', () => {
    mockIPC(() => null);
    const base = snapshotFixture();
    const { rerender } = render(
      <AccountView
        snapshot={{
          ...base,
          account: { ...base.account, launcherAccount: view(), fullGameAllowed: false },
        }}
      />,
    );
    const panel = () =>
      screen.getByRole('heading', { name: 'Launcher account' }).closest('section') as HTMLElement;
    expect(
      within(panel()).getByText(/Offline needs a Microsoft account that owns Minecraft/),
    ).toBeVisible();
    rerender(
      <AccountView
        snapshot={{
          ...base,
          settings: { ...base.settings, launcherAccountOffline: true },
          account: { ...base.account, launcherAccount: view(), fullGameAllowed: true },
        }}
      />,
    );
    expect(within(panel()).getByText(/full game without the account check/)).toBeVisible();
  });
});
