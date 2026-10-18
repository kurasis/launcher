import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it } from 'vitest';
import type { OperationProgress, OperationResult } from '../../lib/contracts';
import { accountFixture, instanceFixture, snapshotFixture } from '../../test/fixtures';
import { InstallPanel } from './InstallPanel';

let launchFails = false;

function backend() {
  launchFails = false;
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === 'install_instance' || cmd === 'repair_instance') return { operationId: 'op-1' };
    if (cmd === 'cancel_operation') return { state: 'cancel_requested' };
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

const progress: OperationProgress = {
  sessionEpoch: 'epoch-1',
  operationId: 'op-1',
  instanceId: '0f1e2d3c-4b5a-4978-8796-a5b4c3d2e1f0',
  kind: 'install',
  sequence: 4,
  revision: 3,
  phase: 'downloading',
  downloadedBytes: 512 * 1024 * 1024,
  totalDownloadBytes: 1024 * 1024 * 1024,
  verifiedFiles: 1200,
  totalFiles: 3961,
  bytesPerSecond: 4 * 1024 * 1024,
  currentFiles: [],
  approval: null,
  timestamp: '2026-10-17T00:00:00Z',
};

function result(overrides: Partial<OperationResult>): OperationResult {
  return {
    operationId: 'op-0',
    instanceId: instanceFixture().id,
    kind: 'install',
    outcome: 'succeeded',
    error: null,
    finishedAt: '2026-10-17T00:00:00Z',
    downloadedFiles: 3961,
    networkBytes: 919_845_786,
    reusedFiles: 0,
    ...overrides,
  };
}

describe('InstallPanel', () => {
  it('says the game is running instead of showing install progress', () => {
    backend();
    render(
      <InstallPanel
        account={accountFixture()}
        instance={instanceFixture({ health: 'ready', busy: true })}
        activeOperation={{ kind: 'launch', instanceId: instanceFixture().id, operationId: 'run-1' }}
        progress={null}
        recentOperations={[]}
      />,
    );
    expect(screen.getByRole('status')).toHaveTextContent('The game is running.');
    expect(screen.queryByRole('button', { name: 'Cancel' })).toBeNull();
    expect(screen.queryByText('Reading version metadata')).toBeNull();
  });

  it('starts an install for a new installation', async () => {
    const calls = backend();
    render(
      <InstallPanel
        account={accountFixture()}
        instance={instanceFixture()}
        activeOperation={null}
        progress={null}
        recentOperations={[]}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Install' }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'install_instance')?.args).toMatchObject({
        request: { payload: { instanceId: instanceFixture().id } },
      });
    });
  });

  it('shows real progress and cancels the running operation', async () => {
    const calls = backend();
    const instance = instanceFixture({ busy: true });
    render(
      <InstallPanel
        account={accountFixture()}
        instance={instance}
        activeOperation={{ kind: 'install', instanceId: instance.id, operationId: 'op-1' }}
        progress={progress}
        recentOperations={[]}
      />,
    );
    expect(screen.getByText('Downloading')).toBeVisible();
    expect(screen.getByRole('progressbar', { name: 'Download progress' })).toHaveAttribute(
      'value',
      String(512 * 1024 * 1024),
    );
    expect(screen.getByText('1,200 of 3,961 files verified')).toBeVisible();
    expect(screen.getByText(/512\.0 MiB of 1\.0 GiB · 4\.0 MiB\/s/)).toBeVisible();
    expect(screen.queryByRole('button', { name: 'Install' })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'cancel_operation')?.args).toMatchObject({
        request: { payload: { operationId: 'op-1' } },
      });
    });
  });

  it('offers retry after a cancelled install and explains kept files', () => {
    backend();
    render(
      <InstallPanel
        account={accountFixture()}
        instance={instanceFixture()}
        activeOperation={null}
        progress={null}
        recentOperations={[result({ outcome: 'cancelled' })]}
      />,
    );
    expect(screen.getByText(/Files verified so far were kept/)).toBeVisible();
    expect(screen.getByRole('button', { name: 'Retry install' })).toBeEnabled();
  });

  it('offers repair for damaged files and verification for ready ones', async () => {
    const calls = backend();
    const { rerender } = render(
      <InstallPanel
        account={accountFixture()}
        instance={instanceFixture({ health: 'repair_required' })}
        activeOperation={null}
        progress={null}
        recentOperations={[]}
      />,
    );
    expect(screen.getByText(/never changes worlds/)).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Repair' }));
    await waitFor(() => {
      expect(calls.some((c) => c.cmd === 'repair_instance')).toBe(true);
    });
    rerender(
      <InstallPanel
        account={accountFixture()}
        instance={instanceFixture({ health: 'ready' })}
        activeOperation={null}
        progress={null}
        recentOperations={[result({})]}
      />,
    );
    expect(screen.getByRole('button', { name: 'Verify files' })).toBeVisible();
    expect(
      screen.getByText(/Installation finished\. Downloaded 3,961 files \(877\.2 MiB\)/),
    ).toBeVisible();
  });

  it('queues installs while another installation is busy', async () => {
    const calls = backend();
    const { rerender } = render(
      <InstallPanel
        account={accountFixture()}
        instance={instanceFixture()}
        activeOperation={{ kind: 'install', instanceId: 'other', operationId: 'op-9' }}
        progress={null}
        recentOperations={[]}
      />,
    );
    expect(screen.getByText(/added to the queue/)).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Add to queue' }));
    await waitFor(() => {
      expect(calls.some((c) => c.cmd === 'install_instance')).toBe(true);
    });
    rerender(
      <InstallPanel
        account={accountFixture()}
        instance={instanceFixture()}
        activeOperation={{ kind: 'install', instanceId: 'other', operationId: 'op-9' }}
        progress={null}
        recentOperations={[]}
        queue={[
          {
            queueId: 'q-1',
            instanceId: instanceFixture().id,
            kind: 'install',
            queuedAt: '2026-10-17T00:00:00Z',
          },
        ]}
      />,
    );
    expect(screen.getByRole('button', { name: 'Add to queue' })).toBeDisabled();
    fireEvent.click(screen.getByRole('button', { name: 'Remove' }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'cancel_operation')?.args).toMatchObject({
        request: { payload: { operationId: 'q-1' } },
      });
    });
  });

  it('offline, plays installed versions and queues downloads for the connection', () => {
    backend();
    const { rerender } = render(
      <InstallPanel
        account={accountFixture()}
        instance={instanceFixture({ health: 'ready' })}
        activeOperation={null}
        progress={null}
        recentOperations={[]}
        network="offline"
      />,
    );
    expect(
      screen.getByText(/No internet connection\. Installed versions still start/),
    ).toBeVisible();
    expect(screen.getByRole('button', { name: 'Play' })).toBeEnabled();
    rerender(
      <InstallPanel
        account={accountFixture()}
        instance={instanceFixture()}
        activeOperation={null}
        progress={null}
        recentOperations={[]}
        network="offline"
        queue={[
          {
            queueId: 'q-1',
            instanceId: instanceFixture().id,
            kind: 'install',
            queuedAt: '2026-10-17T00:00:00Z',
          },
        ]}
      />,
    );
    expect(screen.getByText('Waiting for the internet connection')).toBeVisible();
    expect(screen.getByRole('button', { name: 'Add to queue' })).toBeDisabled();
  });

  it('plays as the default player or the chosen one, always labelled Play', async () => {
    const calls = backend();
    const instance = instanceFixture({ health: 'ready' });
    const { rerender } = render(
      <InstallPanel
        account={accountFixture()}
        instance={instance}
        activeOperation={null}
        progress={null}
        recentOperations={[]}
      />,
    );
    // One "Play" label for every player; the demo hint is gone.
    expect(screen.queryByText(/five in-game days/)).toBeNull();
    expect(screen.getByLabelText('Play as')).toHaveDisplayValue('Default: Player (demo)');
    fireEvent.click(screen.getByRole('button', { name: 'Play' }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'launch_game')?.args).toMatchObject({
        request: { payload: { instanceId: instance.id } },
      });
    });

    const account = accountFixture({
      accounts: [
        {
          profileId: 'steve',
          profileName: 'Steve_Test',
          state: 'verified_access',
          error: null,
        },
      ],
      notokenProfiles: [{ id: 'alex', name: 'Alex' }],
      defaultProfile: { kind: 'microsoft', id: 'steve' },
    });
    rerender(
      <InstallPanel
        account={account}
        instance={instance}
        activeOperation={null}
        progress={null}
        recentOperations={[]}
      />,
    );
    launchFails = true;
    fireEvent.click(screen.getByRole('button', { name: 'Play' }));
    expect(await screen.findByText('Sign-in is not configured in this build.')).toBeVisible();

    // The installation can play as the demo profile instead.
    fireEvent.change(screen.getByLabelText('Play as'), { target: { value: 'notoken:alex' } });
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'set_instance_profile')?.args).toMatchObject({
        request: {
          payload: { instanceId: instance.id, profile: { kind: 'notoken', id: 'alex' } },
        },
      });
    });
    rerender(
      <InstallPanel
        account={account}
        instance={{ ...instance, profile: { kind: 'notoken', id: 'alex' } }}
        activeOperation={null}
        progress={null}
        recentOperations={[]}
      />,
    );
    expect(screen.getByRole('button', { name: 'Play' })).toBeVisible();
    expect(screen.getByLabelText('Play as')).toHaveDisplayValue('Alex (demo)');
  });
});
