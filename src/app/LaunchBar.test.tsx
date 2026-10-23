import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it, vi } from 'vitest';
import type { AppSnapshot, InstanceSummary, OperationResult } from '../lib/contracts';
import { accountFixture, instanceFixture, snapshotFixture } from '../test/fixtures';
import { LaunchBar } from './LaunchBar';

let launchFails = false;

function backend() {
  launchFails = false;
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === 'install_instance' || cmd === 'repair_instance') return { operationId: 'op-1' };
    if (cmd === 'launch_game') {
      if (launchFails) {
        // eslint-disable-next-line @typescript-eslint/only-throw-error -- mirrors the IPC error envelope
        throw {
          code: 'AUTH_NOT_CONFIGURED',
          message: 'Sign-in is not configured in this build.',
          retryable: false,
          diagnosticId: 'd',
        };
      }
      return { runId: 'run-1', kind: 'game', state: 'preflight' };
    }
    if (cmd === 'set_instance_profile') return instanceFixture();
    if (cmd === 'get_app_snapshot') return snapshotFixture();
    return null;
  });
  return calls;
}

/** A snapshot with these installations, the first one chosen. */
function chosen(instances: InstanceSummary[], rest: Partial<AppSnapshot> = {}): AppSnapshot {
  return snapshotFixture({
    instances,
    settings: { ...snapshotFixture().settings, selectedInstanceId: instances[0]?.id ?? null },
    ...rest,
  });
}

const QUEUED = {
  queueId: 'q-1',
  instanceId: instanceFixture().id,
  kind: 'install' as const,
  queuedAt: '2026-10-17T00:00:00Z',
};

function cancelledInstall(): OperationResult {
  return {
    operationId: 'op-0',
    instanceId: instanceFixture().id,
    kind: 'install',
    outcome: 'cancelled',
    error: null,
    finishedAt: '2026-10-17T00:00:00Z',
    downloadedFiles: 10,
    networkBytes: 1000,
    reusedFiles: 0,
  };
}

function bar() {
  return screen.getByRole('region', { name: 'Launch' });
}

describe('LaunchBar', () => {
  it('plays as the default player or the chosen one, always labelled Play', async () => {
    const calls = backend();
    const instance = instanceFixture({ health: 'ready' });
    const { rerender } = render(
      <LaunchBar snapshot={chosen([instance])} onOpenInstallations={vi.fn()} />,
    );
    expect(within(bar()).getByText('Survival')).toBeVisible();
    expect(within(bar()).getByLabelText('Play as')).toHaveDisplayValue('Default: Player');
    fireEvent.click(within(bar()).getByRole('button', { name: 'Play' }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'launch_game')?.args).toMatchObject({
        request: { payload: { instanceId: instance.id } },
      });
    });

    const account = accountFixture({
      accounts: [
        { profileId: 'steve', profileName: 'Steve_Test', state: 'verified_access', error: null },
      ],
      notokenProfiles: [{ id: 'alex', name: 'Alex' }],
      defaultProfile: { kind: 'microsoft', id: 'steve' },
    });
    rerender(
      <LaunchBar snapshot={chosen([instance], { account })} onOpenInstallations={vi.fn()} />,
    );
    launchFails = true;
    fireEvent.click(within(bar()).getByRole('button', { name: 'Play' }));
    expect(await within(bar()).findByRole('alert')).toHaveTextContent(
      'Sign-in is not configured in this build.',
    );
    const options = Array.from(
      within(bar()).getByLabelText<HTMLSelectElement>('Play as').options,
      (o) => o.textContent,
    );
    expect(options).toEqual(['Default: Steve_Test (Microsoft)', 'Alex']);
    fireEvent.change(within(bar()).getByLabelText('Play as'), {
      target: { value: 'notoken:alex' },
    });
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'set_instance_profile')?.args).toMatchObject({
        request: {
          payload: { instanceId: instance.id, profile: { kind: 'notoken', id: 'alex' } },
        },
      });
    });
  });

  it('forgets an error when another installation is chosen', async () => {
    backend();
    const first = instanceFixture({ health: 'ready' });
    const second = instanceFixture({
      id: '1f1e2d3c-4b5a-4978-8796-a5b4c3d2e1f0',
      displayName: 'Creative',
      health: 'ready',
    });
    const { rerender } = render(
      <LaunchBar snapshot={chosen([first, second])} onOpenInstallations={vi.fn()} />,
    );
    launchFails = true;
    fireEvent.click(within(bar()).getByRole('button', { name: 'Play' }));
    expect(await within(bar()).findByRole('alert')).toBeVisible();
    rerender(<LaunchBar snapshot={chosen([second, first])} onOpenInstallations={vi.fn()} />);
    expect(within(bar()).getByText('Creative')).toBeVisible();
    expect(within(bar()).queryByRole('alert')).toBeNull();
  });

  it('installs a new installation and retries an unfinished install', async () => {
    const calls = backend();
    const { rerender } = render(
      <LaunchBar snapshot={chosen([instanceFixture()])} onOpenInstallations={vi.fn()} />,
    );
    expect(within(bar()).queryByRole('button', { name: 'Play' })).toBeNull();
    expect(within(bar()).queryByLabelText('Play as')).toBeNull();
    fireEvent.click(within(bar()).getByRole('button', { name: 'Install' }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'install_instance')?.args).toMatchObject({
        request: { payload: { instanceId: instanceFixture().id } },
      });
    });
    rerender(
      <LaunchBar
        snapshot={chosen([instanceFixture()], { recentOperations: [cancelledInstall()] })}
        onOpenInstallations={vi.fn()}
      />,
    );
    expect(within(bar()).getByRole('button', { name: 'Retry install' })).toBeEnabled();
  });

  it('repairs damaged files', async () => {
    const calls = backend();
    render(
      <LaunchBar
        snapshot={chosen([instanceFixture({ health: 'repair_required' })])}
        onOpenInstallations={vi.fn()}
      />,
    );
    fireEvent.click(within(bar()).getByRole('button', { name: 'Repair' }));
    await waitFor(() => {
      expect(calls.some((c) => c.cmd === 'repair_instance')).toBe(true);
    });
  });

  it('queues an install while another installation is busy', async () => {
    const calls = backend();
    const busy = { kind: 'install' as const, instanceId: 'other', operationId: 'op-9' };
    const { rerender } = render(
      <LaunchBar
        snapshot={chosen([instanceFixture()], { activeOperation: busy })}
        onOpenInstallations={vi.fn()}
      />,
    );
    fireEvent.click(within(bar()).getByRole('button', { name: 'Add to queue' }));
    await waitFor(() => {
      expect(calls.some((c) => c.cmd === 'install_instance')).toBe(true);
    });
    rerender(
      <LaunchBar
        snapshot={chosen([instanceFixture()], { activeOperation: busy, queue: [QUEUED] })}
        onOpenInstallations={vi.fn()}
      />,
    );
    expect(within(bar()).getByRole('button', { name: 'Add to queue' })).toBeDisabled();
    expect(within(bar()).getByRole('status')).toHaveTextContent(/^Queued: starts when/);
  });

  it('offline, plays installed versions and queues downloads for the connection', () => {
    backend();
    const { rerender } = render(
      <LaunchBar
        snapshot={chosen([instanceFixture({ health: 'ready' })], { network: 'offline' })}
        onOpenInstallations={vi.fn()}
      />,
    );
    expect(within(bar()).getByRole('button', { name: 'Play' })).toBeEnabled();
    rerender(
      <LaunchBar
        snapshot={chosen([instanceFixture()], { network: 'offline', queue: [QUEUED] })}
        onOpenInstallations={vi.fn()}
      />,
    );
    expect(within(bar()).getByRole('button', { name: 'Add to queue' })).toBeDisabled();
    expect(within(bar()).getByText('Waiting for the internet connection')).toBeVisible();
  });

  it('shows the running game and its own install as a status, not as a button', () => {
    backend();
    const id = instanceFixture().id;
    const { rerender } = render(
      <LaunchBar
        snapshot={chosen([instanceFixture({ health: 'ready' })], {
          activeOperation: { kind: 'launch', instanceId: id, operationId: 'op-9' },
        })}
        onOpenInstallations={vi.fn()}
      />,
    );
    expect(within(bar()).getByRole('status')).toHaveTextContent('The game is running.');
    expect(within(bar()).queryByRole('button', { name: 'Play' })).toBeNull();
    expect(within(bar()).queryByLabelText('Play as')).toBeNull();
    rerender(
      <LaunchBar
        snapshot={chosen([instanceFixture()], {
          activeOperation: { kind: 'install', instanceId: id, operationId: 'op-1' },
        })}
        onOpenInstallations={vi.fn()}
      />,
    );
    expect(within(bar()).getByRole('status')).toHaveTextContent(/Install/);
    expect(within(bar()).queryByRole('button', { name: /^(Install|Retry install)$/ })).toBeNull();
  });

  it('runs its main action once when asked, for the chosen installation only', async () => {
    const calls = backend();
    const ready = instanceFixture({ health: 'ready' });
    const other = instanceFixture({
      id: '1f1e2d3c-4b5a-4978-8796-a5b4c3d2e1f0',
      displayName: 'Creative',
    });
    const launches = () => calls.filter((c) => c.cmd === 'launch_game').length;
    const installs = () => calls.filter((c) => c.cmd === 'install_instance').length;
    const at = Date.now();
    // Asked for an installation that is not the chosen one (yet): nothing runs.
    const { rerender } = render(
      <LaunchBar snapshot={chosen([ready, other])} request={{ instanceId: other.id, at }} />,
    );
    await act(async () => {
      await Promise.resolve();
    });
    expect(launches() + installs()).toBe(0);
    // It becomes the chosen one: its own main action runs, here Install.
    rerender(
      <LaunchBar snapshot={chosen([other, ready])} request={{ instanceId: other.id, at }} />,
    );
    await waitFor(() => {
      expect(installs()).toBe(1);
    });
    expect(calls.find((c) => c.cmd === 'install_instance')?.args).toMatchObject({
      request: { payload: { instanceId: other.id } },
    });
    // The same request never runs again, not even when the bar comes back to it.
    rerender(
      <LaunchBar snapshot={chosen([ready, other])} request={{ instanceId: other.id, at }} />,
    );
    rerender(
      <LaunchBar snapshot={chosen([other, ready])} request={{ instanceId: other.id, at }} />,
    );
    await act(async () => {
      await Promise.resolve();
    });
    expect(installs()).toBe(1);
    // A new request plays the chosen installed one.
    rerender(
      <LaunchBar
        snapshot={chosen([ready, other])}
        request={{ instanceId: ready.id, at: at + 1 }}
      />,
    );
    await waitFor(() => {
      expect(launches()).toBe(1);
    });
    // An old request (its installation was never chosen in time) is dropped.
    rerender(
      <LaunchBar
        snapshot={chosen([ready, other])}
        request={{ instanceId: ready.id, at: at - 60_000 }}
      />,
    );
    await act(async () => {
      await Promise.resolve();
    });
    expect(launches()).toBe(1);
  });

  it('shows an install of another installation in place of Play, with Cancel', async () => {
    const calls = backend();
    const ready = instanceFixture({ health: 'ready' });
    const other = instanceFixture({
      id: '1f1e2d3c-4b5a-4978-8796-a5b4c3d2e1f0',
      displayName: 'Creative',
      busy: true,
    });
    const view = (phase: 'downloading' | 'cancel_requested') =>
      chosen([ready, other], {
        activeOperation: { kind: 'install', instanceId: other.id, operationId: 'op-7' },
        operationProgress: {
          sessionEpoch: 'epoch-1',
          operationId: 'op-7',
          instanceId: other.id,
          kind: 'install',
          sequence: 1,
          revision: 1,
          phase,
          downloadedBytes: 203 * 1024 * 1024,
          totalDownloadBytes: 460 * 1024 * 1024,
          verifiedFiles: 1240,
          totalFiles: 3120,
          bytesPerSecond: 12 * 1024 * 1024,
          currentFiles: [],
          approval: null,
          timestamp: '',
        },
        queue: [{ ...QUEUED, instanceId: ready.id, kind: 'repair' }],
      });
    const { rerender } = render(<LaunchBar snapshot={view('downloading')} />);
    const bar = screen.getByRole('region', { name: 'Launch' });
    expect(within(bar).getByText('Creative')).toBeVisible();
    expect(within(bar).getByRole('status')).toHaveTextContent('Installation: downloading');
    expect(bar).toHaveTextContent('Installation: downloading, 44%');
    expect(bar).toHaveTextContent('203.0 MiB of 460.0 MiB, 12.0 MiB/s. 1,240 of 3,120 files.');
    expect(within(bar).getByRole('progressbar', { name: 'Download progress' })).toHaveAttribute(
      'value',
      '44',
    );
    expect(within(bar).getByText('Waiting in line: 1')).toBeVisible();
    expect(within(bar).queryByRole('button', { name: /Play/ })).toBeNull();
    fireEvent.click(within(bar).getByRole('button', { name: 'Cancel' }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'cancel_operation')?.args).toMatchObject({
        request: { payload: { operationId: 'op-7' } },
      });
    });
    rerender(<LaunchBar snapshot={view('cancel_requested')} />);
    expect(within(bar).getByRole('button', { name: 'Cancelling…' })).toHaveAttribute(
      'aria-disabled',
      'true',
    );
  });

  it('drops a request that came while another installation installs', async () => {
    const calls = backend();
    const ready = instanceFixture({ health: 'ready' });
    const other = instanceFixture({ id: '1f1e2d3c-4b5a-4978-8796-a5b4c3d2e1f0', busy: true });
    const at = Date.now();
    const installing = chosen([ready, other], {
      activeOperation: { kind: 'install', instanceId: other.id, operationId: 'op-7' },
    });
    const { rerender } = render(
      <LaunchBar snapshot={installing} request={{ instanceId: ready.id, at }} />,
    );
    await act(async () => {
      await Promise.resolve();
    });
    // The install ends: Play comes back, and the old request does not run now.
    rerender(
      <LaunchBar snapshot={chosen([ready, other])} request={{ instanceId: ready.id, at }} />,
    );
    await act(async () => {
      await Promise.resolve();
    });
    expect(screen.getByRole('button', { name: 'Play' })).toBeEnabled();
    expect(calls.filter((c) => c.cmd === 'launch_game')).toHaveLength(0);
  });

  it('shows an install while no installation is chosen', () => {
    backend();
    const other = instanceFixture({ busy: true });
    render(
      <LaunchBar
        snapshot={snapshotFixture({
          instances: [other],
          activeOperation: { kind: 'repair', instanceId: other.id, operationId: 'op-8' },
        })}
      />,
    );
    const bar = screen.getByRole('region', { name: 'Launch' });
    expect(within(bar).getByRole('status')).toHaveTextContent('Repair: reading version metadata');
    expect(within(bar).getByRole('button', { name: 'Cancel' })).toBeVisible();
  });

  it('opens Installations from other screens only', () => {
    backend();
    const open = vi.fn();
    const { rerender } = render(
      <LaunchBar snapshot={snapshotFixture()} onOpenInstallations={open} />,
    );
    expect(within(bar()).getByText('No installation chosen')).toBeVisible();
    fireEvent.click(within(bar()).getByRole('button', { name: 'Open Installations' }));
    expect(open).toHaveBeenCalledOnce();
    fireEvent.click(rerenderChosen(rerender, open));
    expect(open).toHaveBeenCalledTimes(2);

    // On Installations itself the bar links nowhere.
    rerender(<LaunchBar snapshot={snapshotFixture()} />);
    expect(within(bar()).queryByRole('button')).toBeNull();
    rerender(<LaunchBar snapshot={chosen([instanceFixture({ health: 'ready' })])} />);
    expect(within(bar()).queryByRole('button', { name: /Survival/ })).toBeNull();
  });
});

/** Shows a chosen installation and returns the bar's button that opens Installations. */
function rerenderChosen(rerender: (ui: React.ReactElement) => void, open: () => void): HTMLElement {
  rerender(<LaunchBar snapshot={chosen([instanceFixture()])} onOpenInstallations={open} />);
  return within(bar()).getByRole('button', {
    name: 'Open Installations: Survival, Minecraft 1.21.1 · Not installed',
  });
}
