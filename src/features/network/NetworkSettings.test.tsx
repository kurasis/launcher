import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it } from 'vitest';
import type { SettingsView, TrafficSummary } from '../../lib/contracts';
import { snapshotFixture } from '../../test/fixtures';
import { NetworkSettings } from './NetworkSettings';

const MIB = 1024 * 1024;
const traffic: TrafficSummary = {
  month: '2026-10',
  thisMonth: { game: 600 * MIB, java: 50 * MIB, loaders: 10 * MIB, metadata: 2 * MIB, mods: 0 },
  earlier: [{ month: '2026-09', bytes: 2048 * MIB }],
};

function setup(settings: Partial<SettingsView> = {}) {
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === 'get_traffic') return traffic;
    if (cmd === 'reset_traffic')
      return {
        ...traffic,
        thisMonth: { game: 0, java: 0, loaders: 0, metadata: 0, mods: 0 },
        earlier: [],
      };
    if (cmd === 'get_app_snapshot') return snapshotFixture();
    return null;
  });
  const base = snapshotFixture();
  render(<NetworkSettings snapshot={{ ...base, settings: { ...base.settings, ...settings } }} />);
  return calls;
}

const named = (cmd: string, calls: { cmd: string; args: unknown }[]) =>
  calls.filter((c) => c.cmd === cmd);

describe('NetworkSettings', () => {
  it('shows this month against the limit, by source, and earlier months', async () => {
    setup({ metered: true, monthlyLimitMib: 1024 });
    expect(await screen.findByText('662.0 MiB of 1.0 GiB downloaded this month')).toBeVisible();
    expect(screen.getByRole('progressbar')).toHaveAttribute('max', String(1024 * MIB));
    expect(screen.getByText('Game files and Mojang Java').nextSibling).toHaveTextContent(
      '600.0 MiB',
    );
    expect(screen.getByText('Java from Adoptium').nextSibling).toHaveTextContent('50.0 MiB');
    expect(screen.getByText('September 2026').nextSibling).toHaveTextContent('2.0 GiB');
  });

  it('switches metered mode and saves the warning size and limit', async () => {
    const calls = setup();
    fireEvent.click(screen.getByRole('checkbox', { name: /Metered connection/ }));
    await waitFor(() => {
      expect(named('set_app_settings', calls)[0]?.args).toMatchObject({
        request: { payload: { metered: true } },
      });
    });
    fireEvent.change(screen.getByLabelText(/Ask before downloads of at least/), {
      target: { value: '0' },
    });
    fireEvent.change(screen.getByLabelText(/Monthly limit/), { target: { value: '2048' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => {
      expect(named('set_app_settings', calls)[1]?.args).toMatchObject({
        request: { payload: { meteredWarnMib: 0, monthlyLimitMib: 2048 } },
      });
    });
    expect(await screen.findByText('662.0 MiB downloaded this month')).toBeVisible();
    expect(screen.queryByRole('progressbar')).toBeNull();
  });

  it('resets the count after confirmation', async () => {
    const calls = setup();
    fireEvent.click(await screen.findByRole('button', { name: 'Reset the count' }));
    expect(screen.getByText('Forget all traffic counted so far?')).toBeVisible();
    expect(named('reset_traffic', calls)).toHaveLength(0);
    fireEvent.click(screen.getAllByRole('button', { name: 'Reset the count' })[0] as HTMLElement);
    await waitFor(() => {
      expect(named('reset_traffic', calls)).toHaveLength(1);
    });
  });
});
