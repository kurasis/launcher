import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { OperationProgress } from '../../lib/contracts';
import { instanceFixture, snapshotFixture } from '../../test/fixtures';
import { putAside } from '../network/approvalStore';
import { DownloadManager } from './DownloadManager';

const a = instanceFixture({ displayName: 'Survival' });
const b = instanceFixture({
  id: '1f1e2d3c-4b5a-4978-8796-a5b4c3d2e1f0',
  displayName: 'Modded',
  loader: { kind: 'fabric', version: '0.16.14' },
});

const progress: OperationProgress = {
  sessionEpoch: 'epoch-1',
  operationId: 'op-1',
  instanceId: a.id,
  kind: 'install',
  sequence: 3,
  revision: 1,
  phase: 'downloading',
  downloadedBytes: 256 * 1024 * 1024,
  totalDownloadBytes: 1024 * 1024 * 1024,
  verifiedFiles: 1200,
  totalFiles: 4000,
  bytesPerSecond: 8 * 1024 * 1024,
  currentFiles: ['client.jar', 'lwjgl-3.3.3.jar'],
  approval: null,
  timestamp: '2026-10-17T00:00:00Z',
};

afterEach(() => {
  vi.useRealTimers();
});

describe('DownloadManager', () => {
  it('shows the running download, its files and the queue', async () => {
    vi.useFakeTimers();
    const calls: { cmd: string; args: unknown }[] = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === 'cancel_operation') return { state: 'removed' };
      if (cmd === 'get_app_snapshot') return snapshotFixture();
      return null;
    });
    render(
      <DownloadManager
        snapshot={snapshotFixture({
          instances: [a, b],
          activeOperation: { kind: 'install', instanceId: a.id, operationId: 'op-1' },
          operationProgress: progress,
          queue: [
            { queueId: 'q-1', instanceId: b.id, kind: 'repair', queuedAt: '2026-10-17T00:00:00Z' },
          ],
        })}
      />,
    );
    const button = screen.getByRole('button', { name: 'Downloads' });
    expect(button).toHaveAttribute('data-working', 'true');
    expect(button).toHaveTextContent('1');
    fireEvent.click(button);
    expect(screen.getByText('Survival')).toBeVisible();
    expect(screen.getByText('Minecraft 1.21.1')).toBeVisible();
    expect(screen.getByText(/Downloading · 25%/)).toBeVisible();
    expect(
      screen.getByText(/256\.0 MiB of 1\.0 GiB · 8\.0 MiB\/s · 1,200 of 4,000 files/),
    ).toBeVisible();
    expect(screen.getByText('Downloading client.jar')).toBeVisible();
    act(() => {
      vi.advanceTimersByTime(1300);
    });
    expect(screen.getByText('Downloading lwjgl-3.3.3.jar')).toBeVisible();
    expect(screen.getByText('Waiting in line: 1')).toBeVisible();
    expect(screen.getByText('Modded')).toBeVisible();
    expect(screen.getByText('Minecraft 1.21.1 · Fabric 0.16.14 · Repair')).toBeVisible();
    vi.useRealTimers();
    fireEvent.click(screen.getByRole('button', { name: 'Remove' }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'cancel_operation')?.args).toMatchObject({
        request: { payload: { operationId: 'q-1' } },
      });
    });
  });

  it('keeps a file name readable between progress updates', () => {
    vi.useFakeTimers();
    mockIPC(() => null);
    const view = (currentFiles: string[]) => (
      <DownloadManager
        snapshot={snapshotFixture({
          instances: [a],
          activeOperation: { kind: 'install', instanceId: a.id, operationId: 'op-1' },
          operationProgress: { ...progress, currentFiles },
        })}
      />
    );
    const { rerender } = render(view(['a.ogg', 'b.ogg']));
    fireEvent.click(screen.getByRole('button', { name: 'Downloads' }));
    expect(screen.getByText('Downloading a.ogg')).toBeVisible();
    rerender(view(['b.ogg', 'a.ogg', 'c.ogg']));
    expect(screen.getByText('Downloading a.ogg')).toBeVisible();
    act(() => {
      vi.advanceTimersByTime(1300);
    });
    expect(screen.getByText('Downloading c.ogg')).toBeVisible();
    rerender(view([]));
    expect(screen.queryByText(/Downloading .*\.ogg/)).toBeNull();
  });

  it('marks a download that waits for permission, and shows its question again', () => {
    mockIPC((cmd) => (cmd === 'get_app_snapshot' ? snapshotFixture() : null));
    render(
      <DownloadManager
        snapshot={snapshotFixture({
          instances: [a],
          activeOperation: { kind: 'install', instanceId: a.id, operationId: 'op-asks' },
          operationProgress: {
            ...progress,
            operationId: 'op-asks',
            phase: 'awaiting_approval',
            downloadedBytes: 0,
            currentFiles: [],
            approval: {
              bytes: 550 * 1024 * 1024,
              files: 3955,
              reason: 'large',
              monthBytes: 0,
              monthlyLimitMib: 0,
            },
          },
        })}
      />,
    );
    const button = screen.getByRole('button', { name: 'Downloads' });
    expect(button).toHaveAttribute('data-asking', 'true');
    // The question is a dialog of its own (MeteredApproval): the panel stays closed.
    expect(screen.queryByRole('region', { name: 'Downloads' })).toBeNull();
    fireEvent.click(button);
    expect(screen.getByRole('region', { name: 'Downloads' })).toBeVisible();
    // Not put aside: nothing to show again.
    expect(screen.queryByRole('button', { name: 'Show the question' })).toBeNull();
    act(() => {
      putAside('op-asks');
    });
    fireEvent.click(screen.getByRole('button', { name: 'Show the question' }));
    expect(screen.queryByRole('button', { name: 'Show the question' })).toBeNull();
  });

  it('counts checked files before anything downloads', () => {
    mockIPC(() => null);
    render(
      <DownloadManager
        snapshot={snapshotFixture({
          instances: [a],
          activeOperation: { kind: 'repair', instanceId: a.id, operationId: 'op-1' },
          operationProgress: {
            ...progress,
            kind: 'repair',
            phase: 'checking_files',
            downloadedBytes: 0,
            totalDownloadBytes: 0,
            verifiedFiles: 1000,
            bytesPerSecond: null,
            currentFiles: [],
          },
        })}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Downloads' }));
    expect(screen.getByText('Repair')).toBeVisible();
    expect(screen.getByText(/Checking files · 25%/)).toBeVisible();
    // No leading separator when there are no bytes to show.
    expect(screen.getByText('1,000 of 4,000 files')).toBeVisible();
  });

  it('is idle without downloads', () => {
    mockIPC(() => null);
    render(<DownloadManager snapshot={snapshotFixture()} />);
    const button = screen.getByRole('button', { name: 'Downloads' });
    expect(button).not.toHaveAttribute('data-working');
    fireEvent.click(button);
    expect(screen.getByText('Nothing is downloading.')).toBeVisible();
    fireEvent.keyDown(button, { key: 'Escape' });
    expect(screen.queryByText('Nothing is downloading.')).toBeNull();
  });
});
