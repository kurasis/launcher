import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it, vi } from 'vitest';
import type { SettingsView, StatsSent } from '../../lib/contracts';
import { snapshotFixture } from '../../test/fixtures';
import { PrivacySettings } from './PrivacySettings';
import { StatsNotice } from './StatsNotice';

type Call = { cmd: string; args: Record<string, unknown> };

function mock(answers: Record<string, (args: Record<string, unknown>) => unknown>) {
  const calls: Call[] = [];
  mockIPC((cmd, args) => {
    const given = (args ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args: given });
    const answer = answers[cmd];
    return answer === undefined ? null : answer(given);
  });
  return calls;
}

function patches(calls: Call[]): unknown[] {
  return calls
    .filter((c) => c.cmd === 'set_app_settings')
    .map((c) => (c.args.request as { payload: unknown }).payload);
}

const base = snapshotFixture().settings;

const ping = {
  schema: 1,
  day: '2026-10-22',
  first: ['day'],
  cohort: '2026-W43',
  launcher: '1.4.0',
  os: 'windows',
  ui: 'ru',
};

describe('PrivacySettings', () => {
  it('turns the statistics off and makes a new install ID', async () => {
    const calls = mock({ get_app_snapshot: () => snapshotFixture() });
    const { rerender } = render(<PrivacySettings settings={base} />);
    expect(screen.getByText('Not created yet: it appears with your first report.')).toBeVisible();
    const toggle = screen.getByRole('switch', { name: /Anonymous statistics/ });
    // On unless the player turned it off (D-071).
    expect(toggle).toBeChecked();
    // What is counted is read with it, as its description.
    expect(toggle).toHaveAccessibleName('Anonymous statistics');
    expect(toggle).toHaveAccessibleDescription(/no addresses/);
    fireEvent.click(toggle);
    await waitFor(() => {
      expect(patches(calls)).toEqual([{ stats: false, statsNoticeSeen: true }]);
    });
    rerender(
      <PrivacySettings
        settings={{ ...base, stats: false, installId: '9b2f1c3e-0000-4000-8000-000000000001' }}
      />,
    );
    expect(screen.getByRole('switch', { name: /Anonymous statistics/ })).not.toBeChecked();
    expect(screen.getByText('9b2f1c3e-0000-4000-8000-000000000001')).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Make a new ID' }));
    await waitFor(() => {
      expect(patches(calls)[1]).toEqual({ newInstallId: true });
    });
  });

  it('shows what is sent word for word, as text', async () => {
    const sent: StatsSent = {
      ping: { ...ping, launcher: '<img src=x onerror=alert(1)>' },
      counts: { schema: 1, day: '2026-10-22', ads: { '7': { shown: 3 } } },
      next: null,
      waiting: 2,
    };
    const calls = mock({ stats_sent: () => sent });
    render(<PrivacySettings settings={base} />);
    const button = screen.getByRole('button', { name: 'What is sent' });
    expect(button).toHaveAttribute('aria-expanded', 'false');
    expect(calls.some((c) => c.cmd === 'stats_sent')).toBe(false);
    fireEvent.click(button);
    expect(button).toHaveAttribute('aria-expanded', 'true');
    const daily = await screen.findByText('Last daily count');
    const packet = within(daily.parentElement as HTMLElement).getByText(/"cohort": "2026-W43"/);
    expect(packet.tagName).toBe('PRE');
    expect(packet.textContent).toContain('"launcher": "<img src=x onerror=alert(1)>"');
    expect(document.querySelector('img[src="x"]')).toBeNull();
    expect(screen.getByText('Last ad counts')).toBeVisible();
    expect(screen.queryByText('Goes next')).toBeNull();
    expect(screen.getByText('2 packets wait to be sent.')).toBeVisible();
    fireEvent.click(button);
    expect(screen.queryByText('Last daily count')).toBeNull();
  });

  it("shows today's count before it goes, and nothing once turned off", async () => {
    let on = true;
    const calls = mock({
      stats_sent: () =>
        on
          ? { ping: null, counts: null, next: ping, waiting: 0 }
          : { ping: null, counts: null, next: null, waiting: 0 },
      set_app_settings: () => {
        on = false;
        return { ...base, stats: false };
      },
    });
    const { rerender } = render(<PrivacySettings settings={base} />);
    fireEvent.click(screen.getByRole('button', { name: 'What is sent' }));
    expect(await screen.findByText('Goes next')).toBeVisible();
    expect(screen.queryByText('Nothing has been sent yet.')).toBeNull();
    fireEvent.click(screen.getByRole('switch', { name: /Anonymous statistics/ }));
    await waitFor(() => {
      expect(calls.filter((c) => c.cmd === 'stats_sent')).toHaveLength(2);
    });
    rerender(<PrivacySettings settings={{ ...base, stats: false }} />);
    expect(await screen.findByText('Statistics are off: nothing is sent.')).toBeVisible();
    expect(screen.queryByText('Goes next')).toBeNull();
  });

  it('says when nothing has been sent yet, and when the list cannot be read', async () => {
    let fail = false;
    mock({
      stats_sent: () => {
        if (fail) {
          // The Rust core rejects with a plain error envelope, not an Error instance.
          // eslint-disable-next-line @typescript-eslint/only-throw-error
          throw {
            code: 'STORAGE_ERROR',
            message: 'The statistics could not be read.',
            retryable: false,
            diagnosticId: 'diag-1',
          };
        }
        return { ping: null, counts: null, next: null, waiting: 0 };
      },
    });
    render(<PrivacySettings settings={base} />);
    const button = screen.getByRole('button', { name: 'What is sent' });
    fireEvent.click(button);
    expect(await screen.findByText('Nothing has been sent yet.')).toBeVisible();
    fireEvent.click(button);
    fail = true;
    fireEvent.click(button);
    expect(await screen.findByText(/could not be read/)).toBeVisible();
  });
});

describe('StatsNotice', () => {
  const unseen: SettingsView = { ...base, stats: true, statsNoticeSeen: false };

  it('says what the statistics are until Got it, and is not shown again', async () => {
    const calls = mock({});
    const onSettings = vi.fn();
    const { rerender } = render(<StatsNotice settings={unseen} onSettings={onSettings} />);
    expect(
      screen.getByText(
        'The launcher sends anonymous statistics without any ID: versions, system, country, what is launched, ad impressions.',
      ),
    ).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Got it' }));
    expect(screen.queryByRole('button', { name: 'Got it' })).toBeNull();
    await waitFor(() => {
      expect(patches(calls)).toEqual([{ statsNoticeSeen: true }]);
    });
    expect(onSettings).not.toHaveBeenCalled();
    rerender(
      <StatsNotice settings={{ ...unseen, statsNoticeSeen: true }} onSettings={onSettings} />,
    );
    expect(screen.queryByRole('button', { name: 'Got it' })).toBeNull();
  });

  it('opens the privacy settings', async () => {
    const calls = mock({});
    const onSettings = vi.fn();
    render(<StatsNotice settings={unseen} onSettings={onSettings} />);
    const button = screen.getByRole('button', { name: 'Privacy settings' });
    expect(button).toHaveTextContent('Settings');
    fireEvent.click(button);
    expect(onSettings).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole('button', { name: 'Privacy settings' })).toBeNull();
    await waitFor(() => {
      expect(patches(calls)).toEqual([{ statsNoticeSeen: true }]);
    });
  });

  it('is not shown when read, turned off, or when nothing can be saved', () => {
    mock({});
    for (const settings of [
      { ...unseen, statsNoticeSeen: true },
      { ...unseen, stats: false },
      { ...unseen, writable: false },
    ]) {
      const { unmount } = render(<StatsNotice settings={settings} onSettings={() => undefined} />);
      expect(screen.queryByRole('button', { name: 'Got it' })).toBeNull();
      unmount();
    }
  });
});
