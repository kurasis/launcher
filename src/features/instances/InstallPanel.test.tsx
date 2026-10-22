import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it } from 'vitest';
import type { OperationProgress, OperationResult } from '../../lib/contracts';
import { instanceFixture, snapshotFixture } from '../../test/fixtures';
import { InstallPanel } from './InstallPanel';

function backend() {
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === 'install_instance' || cmd === 'repair_instance') return { operationId: 'op-1' };
    if (cmd === 'cancel_operation') return { state: 'cancel_requested' };
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

  it('leaves Play, Install, Repair and "Play as" to the launch bar', () => {
    backend();
    const { rerender } = render(
      <InstallPanel
        instance={instanceFixture({ health: 'ready' })}
        activeOperation={null}
        progress={null}
        recentOperations={[]}
      />,
    );
    expect(screen.queryByRole('button', { name: 'Play' })).toBeNull();
    expect(screen.queryByLabelText('Play as')).toBeNull();
    for (const health of ['not_installed', 'repair_required'] as const) {
      rerender(
        <InstallPanel
          instance={instanceFixture({ health })}
          activeOperation={null}
          progress={null}
          recentOperations={[]}
        />,
      );
      expect(screen.queryByRole('button', { name: /Install|Repair/ })).toBeNull();
    }
  });

  it('shows real progress and cancels the running operation', async () => {
    const calls = backend();
    const instance = instanceFixture({ busy: true });
    render(
      <InstallPanel
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
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'cancel_operation')?.args).toMatchObject({
        request: { payload: { operationId: 'op-1' } },
      });
    });
  });

  it('explains the files kept by a cancelled install', () => {
    backend();
    render(
      <InstallPanel
        instance={instanceFixture()}
        activeOperation={null}
        progress={null}
        recentOperations={[result({ outcome: 'cancelled' })]}
      />,
    );
    expect(screen.getByText(/Files verified so far were kept/)).toBeVisible();
  });

  it('explains repair for damaged files and verifies ready ones', async () => {
    const calls = backend();
    const { rerender } = render(
      <InstallPanel
        instance={instanceFixture({ health: 'repair_required' })}
        activeOperation={null}
        progress={null}
        recentOperations={[]}
      />,
    );
    expect(screen.getByText(/never changes worlds/)).toBeVisible();
    rerender(
      <InstallPanel
        instance={instanceFixture({ health: 'ready' })}
        activeOperation={null}
        progress={null}
        recentOperations={[result({})]}
      />,
    );
    expect(
      screen.getByText(/Installation finished\. Downloaded 3,961 files \(877\.2 MiB\)/),
    ).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Verify files' }));
    await waitFor(() => {
      expect(calls.some((c) => c.cmd === 'repair_instance')).toBe(true);
    });
  });

  it('shows the queue while another installation is busy, and removes from it', async () => {
    const calls = backend();
    const { rerender } = render(
      <InstallPanel
        instance={instanceFixture()}
        activeOperation={{ kind: 'install', instanceId: 'other', operationId: 'op-9' }}
        progress={null}
        recentOperations={[]}
      />,
    );
    expect(screen.getByText(/added to the queue/)).toBeVisible();
    rerender(
      <InstallPanel
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
    fireEvent.click(screen.getByRole('button', { name: 'Remove' }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'cancel_operation')?.args).toMatchObject({
        request: { payload: { operationId: 'q-1' } },
      });
    });
  });

  it('says offline what still works and what waits for the connection', () => {
    backend();
    const { rerender } = render(
      <InstallPanel
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
    rerender(
      <InstallPanel
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
  });
});
