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
    if (cmd === 'launch_game') {
      // The no-token profile needs no account; a normal launch fails without sign-in setup.
      if ((args as { request: { payload: { notoken: boolean } } }).request.payload.notoken) {
        return { runId: 'run-1', kind: 'game', state: 'preflight' };
      }
      // eslint-disable-next-line @typescript-eslint/only-throw-error -- mirrors the IPC error envelope
      throw {
        code: 'AUTH_NOT_CONFIGURED',
        message: 'Sign-in is not configured in this build.',
        retryable: false,
        diagnosticId: 'd',
      };
    }
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
        accountState="signed_out"
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
        accountState="signed_out"
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
        accountState="signed_out"
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
        accountState="signed_out"
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
        accountState="signed_out"
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
        accountState="signed_out"
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

  it('is disabled while another installation is busy', () => {
    backend();
    render(
      <InstallPanel
        accountState="signed_out"
        instance={instanceFixture()}
        activeOperation={{ kind: 'install', instanceId: 'other', operationId: 'op-9' }}
        progress={null}
        recentOperations={[]}
      />,
    );
    expect(screen.getByRole('button', { name: 'Install' })).toBeDisabled();
    expect(screen.getByText(/Another operation is running/)).toBeVisible();
  });

  it('offers the no-token demo without an account and Play with verified access', async () => {
    const calls = backend();
    const { rerender } = render(
      <InstallPanel
        accountState="signed_out"
        instance={instanceFixture({ health: 'ready' })}
        activeOperation={null}
        progress={null}
        recentOperations={[]}
      />,
    );
    expect(screen.queryByRole('button', { name: 'Play' })).toBeNull();
    expect(screen.getByText(/five in-game days, no multiplayer/)).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Play demo' }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'launch_game')?.args).toMatchObject({
        request: { payload: { instanceId: instanceFixture().id, notoken: true } },
      });
    });

    rerender(
      <InstallPanel
        accountState="verified_access"
        instance={instanceFixture({ health: 'ready' })}
        activeOperation={null}
        progress={null}
        recentOperations={[]}
      />,
    );
    expect(screen.queryByText(/five in-game days/)).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Play' }));
    expect(await screen.findByText('Sign-in is not configured in this build.')).toBeVisible();
    expect(calls.filter((c) => c.cmd === 'launch_game').at(-1)?.args).toMatchObject({
      request: { payload: { instanceId: instanceFixture().id, notoken: false } },
    });
  });
});
