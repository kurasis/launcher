import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
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
    expect(screen.getByRole('progressbar', { name: 'Traffic this month' })).toHaveAttribute(
      'max',
      String(1024 * MIB),
    );
    const row = (name: string) => screen.getByText(name).closest('li') as HTMLElement;
    expect(row('Game files and Mojang Java')).toHaveTextContent('600.0 MiB');
    expect(row('Java from Adoptium')).toHaveTextContent('50.0 MiB');
    // The sources in the board's order; each bar is its share of the month, for the eye
    // only (the size says it).
    const sources = within(row('Mod loaders').parentElement as HTMLElement).getAllByRole(
      'listitem',
    );
    expect(sources.map((item) => item.firstChild?.textContent)).toEqual([
      'Game files and Mojang Java',
      'Java from Adoptium',
      'Mod loaders',
      'Version lists',
      'Catalog of mods',
    ]);
    const share = row('Game files and Mojang Java').querySelector('progress');
    expect(share).toHaveAttribute('value', String(600 * MIB));
    expect(share).toHaveAttribute('max', String(662 * MIB));
    // Earlier months wait behind a press.
    const earlier = screen.getByRole('button', { name: 'Earlier months' });
    expect(earlier).toHaveAttribute('aria-expanded', 'false');
    expect(screen.getByText('September 2026')).not.toBeVisible();
    fireEvent.click(earlier);
    expect(earlier).toHaveAttribute('aria-expanded', 'true');
    expect(row('September 2026')).toHaveTextContent('2.0 GiB');
    expect(row('September 2026')).toBeVisible();
  });

  it('marks a month past its limit', async () => {
    setup({ metered: true, monthlyLimitMib: 512 });
    expect(await screen.findByText('662.0 MiB of 512.0 MiB downloaded this month')).toBeVisible();
    const bar = screen.getByRole('progressbar', { name: 'Traffic this month' });
    expect(bar).toHaveAttribute('value', String(512 * MIB));
    expect(bar.className).toMatch(/over/);
  });

  it('switches metered mode and saves the warning size and limit', async () => {
    const calls = setup();
    const toggle = screen.getByRole('switch', { name: 'Metered connection' });
    // The hint describes the switch; it is not part of its name.
    expect(toggle).toHaveAccessibleDescription(/Ask before big downloads/);
    fireEvent.click(toggle);
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
    const reset = await screen.findByRole('button', { name: 'Reset the count' });
    reset.focus();
    fireEvent.click(reset);
    expect(screen.getByText('Forget all traffic counted so far?')).toBeVisible();
    // The question takes the focus; Cancel gives it back to the button.
    expect(screen.getByRole('alertdialog', { name: 'Reset the count' })).toHaveFocus();
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(screen.getByRole('button', { name: 'Reset the count' })).toHaveFocus();
    fireEvent.click(screen.getByRole('button', { name: 'Reset the count' }));
    expect(named('reset_traffic', calls)).toHaveLength(0);
    fireEvent.click(screen.getAllByRole('button', { name: 'Reset the count' })[0] as HTMLElement);
    await waitFor(() => {
      expect(named('reset_traffic', calls)).toHaveLength(1);
    });
  });
});
