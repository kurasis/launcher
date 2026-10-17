import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it } from 'vitest';
import type { AccountSummary, AppSnapshot } from '../../lib/contracts';
import { snapshotFixture } from '../../test/fixtures';
import { AccountView } from './AccountView';

function setup(account: Partial<AccountSummary>, overrides: Partial<AppSnapshot> = {}) {
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === 'begin_login') return { operationId: 'op-1' };
    if (cmd === 'get_app_snapshot') return snapshotFixture();
    return null;
  });
  const base = snapshotFixture({ microsoftClientId: 'configured_unverified', ...overrides });
  render(<AccountView snapshot={{ ...base, account: { ...base.account, ...account } }} />);
  return calls;
}

const named = (cmd: string, calls: { cmd: string; args: unknown }[]) =>
  calls.filter((c) => c.cmd === cmd);

describe('AccountView', () => {
  it('explains a missing client ID instead of offering sign-in', () => {
    setup({}, { microsoftClientId: 'not_configured' });
    expect(screen.getByText(/no project-owned Microsoft application ID/)).toBeVisible();
    expect(screen.queryByRole('button', { name: /Sign in/ })).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/password/i)).not.toBeInTheDocument();
  });

  it('starts browser sign-in through the backend', async () => {
    const calls = setup({});
    fireEvent.click(screen.getByRole('button', { name: 'Sign in with Microsoft' }));
    await waitFor(() => {
      expect(named('begin_login', calls)).toHaveLength(1);
    });
    expect(screen.getByText(/never sees your password/)).toBeVisible();
  });

  it('offers cancel while waiting for the browser', async () => {
    const calls = setup({ state: 'signing_in', loginOperationId: 'op-7' });
    expect(screen.getByRole('status')).toHaveTextContent('Finish signing in');
    fireEvent.click(screen.getByRole('button', { name: 'Cancel sign-in' }));
    await waitFor(() => {
      expect(named('cancel_login', calls)[0]?.args).toMatchObject({
        request: { payload: { operationId: 'op-7' } },
      });
    });
  });

  it('shows the verified profile and signs out', async () => {
    const calls = setup({ state: 'verified_access', profileName: 'Steve_Test', profileId: 'x' });
    expect(screen.getByText('Steve_Test')).toBeVisible();
    expect(screen.getByText(/access verified/)).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Sign out' }));
    await waitFor(() => {
      expect(named('sign_out', calls)).toHaveLength(1);
    });
  });

  it('keeps no-access distinct from service outage', async () => {
    const calls = setup({
      state: 'no_access',
      profileName: 'Steve_Test',
      error: {
        code: 'NO_GAME_ACCESS',
        message: 'This Microsoft account does not currently have access.',
        retryable: false,
        diagnosticId: 'd',
      },
    });
    expect(screen.getByText('No Java Edition access')).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Get Minecraft' }));
    await waitFor(() => {
      expect(named('open_external', calls)[0]?.args).toEqual({ link: 'buy_minecraft' });
    });
  });

  it('service outage offers a re-check, not a purchase', () => {
    setup({
      state: 'service_unavailable',
      profileName: 'Steve_Test',
      error: {
        code: 'SERVICE_UNAVAILABLE',
        message: 'temporarily unavailable',
        retryable: true,
        diagnosticId: 'd',
      },
    });
    expect(screen.getByRole('button', { name: 'Check again' })).toBeVisible();
    expect(screen.queryByRole('button', { name: 'Get Minecraft' })).not.toBeInTheDocument();
  });

  it('reports incomplete sign-out and session-only mode', () => {
    setup({ state: 'sign_out_pending', sessionOnly: true });
    expect(screen.getByRole('button', { name: 'Try signing out again' })).toBeVisible();
    expect(screen.getByText(/credential store is unavailable/)).toBeVisible();
  });
});
