import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { OperationProgress, OperationResult } from '../../lib/contracts';
import { instanceFixture, snapshotFixture } from '../../test/fixtures';
import { putAside } from '../network/approvalStore';
import { MeteredApproval } from '../network/MeteredApproval';
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

/** The panel's text with the spaces a screen shows (the file name is a span of its own). */
function text(element: HTMLElement) {
  return element.textContent.replace(/\s+/g, ' ').trim();
}

const done: OperationResult = {
  operationId: 'op-0',
  instanceId: b.id,
  kind: 'install',
  outcome: 'succeeded',
  error: null,
  finishedAt: new Date(Date.now() - 86_400_000).toISOString(),
  downloadedFiles: 3120,
  networkBytes: 480_000_000,
  reusedFiles: 0,
};

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
    // The row says how far it got, and how many wait.
    const button = screen.getByRole('button', { name: 'Downloads 25%' });
    expect(button).toHaveAttribute('data-working', 'true');
    expect(button).toHaveTextContent('1');
    fireEvent.click(button);
    const panel = screen.getByRole('region', { name: 'Downloads' });
    expect(screen.getByRole('heading', { name: 'Downloads' })).toHaveFocus();
    expect(screen.getByText('Survival')).toBeVisible();
    expect(screen.getByText('Installation: downloading, 25%')).toBeVisible();
    expect(screen.getByRole('progressbar', { name: 'Download progress' })).toHaveAttribute(
      'value',
      '25',
    );
    expect(text(panel)).toContain(
      '256.0 MiB of 1.0 GiB, 8.0 MiB/s. 1,200 of 4,000 files. Downloading client.jar',
    );
    expect(screen.getByText('client.jar')).toHaveAttribute('dir', 'ltr');
    act(() => {
      vi.advanceTimersByTime(1300);
    });
    expect(screen.getByText('lwjgl-3.3.3.jar')).toBeVisible();
    expect(screen.getByRole('heading', { name: 'Waiting in line: 1' })).toBeVisible();
    expect(screen.getByText('Modded')).toBeVisible();
    expect(screen.getByText('Repair. Starts when the current operation ends.')).toBeVisible();
    vi.useRealTimers();
    fireEvent.click(screen.getByRole('button', { name: 'Remove “Modded” from the queue' }));
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'cancel_operation')?.args).toMatchObject({
        request: { payload: { operationId: 'q-1' } },
      });
    });
  });

  it('cancels the running operation, once', async () => {
    const calls: string[] = [];
    mockIPC((cmd, args) => {
      if (cmd === 'cancel_operation') {
        const { request } = args as { request: { payload: { operationId: string } } };
        calls.push(request.payload.operationId);
        return { state: 'cancel_requested' };
      }
      return null;
    });
    const view = (phase: OperationProgress['phase']) => (
      <DownloadManager
        snapshot={snapshotFixture({
          instances: [a],
          activeOperation: { kind: 'install', instanceId: a.id, operationId: 'op-1' },
          operationProgress: { ...progress, phase },
        })}
      />
    );
    const { rerender } = render(view('downloading'));
    fireEvent.click(screen.getByRole('button', { name: /^Downloads/ }));
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    await waitFor(() => {
      expect(calls).toEqual(['op-1']);
    });
    rerender(view('cancel_requested'));
    const cancelling = screen.getByRole('button', { name: 'Cancelling…' });
    expect(cancelling).toHaveAttribute('aria-disabled', 'true');
    fireEvent.click(cancelling);
    expect(calls).toEqual(['op-1']);
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
    fireEvent.click(screen.getByRole('button', { name: /^Downloads/ }));
    expect(screen.getByText('a.ogg')).toBeVisible();
    rerender(view(['b.ogg', 'a.ogg', 'c.ogg']));
    expect(screen.getByText('a.ogg')).toBeVisible();
    act(() => {
      vi.advanceTimersByTime(1300);
    });
    expect(screen.getByText('c.ogg')).toBeVisible();
    rerender(view([]));
    expect(screen.queryByText(/\.ogg/)).toBeNull();
  });

  it('marks a download that waits for permission, and shows its question again', () => {
    mockIPC((cmd) => (cmd === 'get_app_snapshot' ? snapshotFixture() : null));
    const snapshot = snapshotFixture({
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
    });
    render(
      <>
        <DownloadManager snapshot={snapshot} />
        <MeteredApproval snapshot={snapshot} />
      </>,
    );
    const button = screen.getByRole('button', { name: /^Downloads/ });
    expect(button).toHaveAttribute('data-asking', 'true');
    // The question is a dialog of its own: the panel stays closed. Put aside, it can be
    // shown again from the panel.
    expect(screen.getByRole('dialog', { name: 'Download 550.0 MiB?' })).toBeVisible();
    expect(screen.queryByRole('region', { name: 'Downloads' })).toBeNull();
    act(() => {
      putAside('op-asks');
    });
    expect(screen.queryByRole('dialog')).toBeNull();
    fireEvent.click(button);
    expect(screen.getByRole('region', { name: 'Downloads' })).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Show the question' }));
    expect(screen.getByRole('dialog', { name: 'Download 550.0 MiB?' })).toBeVisible();
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
    fireEvent.click(screen.getByRole('button', { name: 'Downloads 25%' }));
    expect(screen.getByText('Repair: checking files, 25%')).toBeVisible();
    // Only the sentences that are known.
    expect(screen.getByText('1,000 of 4,000 files.')).toBeVisible();
  });

  it('says how recent operations ended', () => {
    mockIPC(() => null);
    render(
      <DownloadManager
        snapshot={snapshotFixture({
          instances: [a, b],
          recentOperations: [
            done,
            {
              ...done,
              operationId: 'op-f',
              instanceId: null,
              kind: 'repair',
              outcome: 'cancelled',
              finishedAt: new Date().toISOString(),
              downloadedFiles: 0,
            },
          ],
        })}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Downloads' }));
    const recent = screen.getByRole('list', { name: '' });
    expect(text(recent)).toContain(
      'ModdedSucceededInstallation finished yesterday. Downloaded 457.8 MiB, 3,120 files.',
    );
    // A removed installation keeps its row, without a tile.
    expect(text(recent)).toContain('Removed installationCancelledRepair cancelled today.');
  });

  it('names the version of installations that share a name', () => {
    mockIPC(() => null);
    const twin = instanceFixture({
      id: '2f1e2d3c-4b5a-4978-8796-a5b4c3d2e1f0',
      displayName: 'Survival',
      versionId: '1.20.1',
    });
    render(
      <DownloadManager
        snapshot={snapshotFixture({
          instances: [a, twin],
          queue: [{ queueId: 'q-1', instanceId: twin.id, kind: 'install', queuedAt: '' }],
          network: 'offline',
        })}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Downloads' }));
    expect(screen.getByText('Minecraft 1.20.1')).toBeVisible();
    expect(screen.getByText('Installation. Waits for the internet connection.')).toBeVisible();
  });

  it('shows the month on a metered connection', async () => {
    mockIPC((cmd) =>
      cmd === 'get_traffic'
        ? {
            month: '2026-10',
            thisMonth: { game: 1024 ** 3, java: 0, loaders: 0, metadata: 0, mods: 0 },
            earlier: [],
          }
        : null,
    );
    const base = snapshotFixture();
    const { rerender } = render(
      <DownloadManager
        snapshot={{ ...base, settings: { ...base.settings, metered: true, monthlyLimitMib: 5120 } }}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Downloads' }));
    expect(
      await screen.findByText('Metered connection: 1.0 GiB of 5.0 GiB downloaded this month.'),
    ).toBeVisible();
    rerender(
      <DownloadManager
        snapshot={{ ...base, settings: { ...base.settings, metered: true, monthlyLimitMib: 0 } }}
      />,
    );
    expect(screen.getByText('Metered connection: 1.0 GiB downloaded this month.')).toBeVisible();
    rerender(<DownloadManager snapshot={base} />);
    expect(screen.queryByText(/Metered connection/)).toBeNull();
  });

  it('is idle without downloads, and closes back to its row', () => {
    mockIPC(() => null);
    render(<DownloadManager snapshot={snapshotFixture()} />);
    const button = screen.getByRole('button', { name: 'Downloads' });
    expect(button).not.toHaveAttribute('data-working');
    fireEvent.click(button);
    expect(screen.getByText('Nothing is downloading.')).toBeVisible();
    fireEvent.keyDown(screen.getByRole('heading', { name: 'Downloads' }), { key: 'Escape' });
    expect(screen.queryByText('Nothing is downloading.')).toBeNull();
    expect(button).toHaveFocus();
    fireEvent.click(button);
    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    expect(screen.queryByRole('region', { name: 'Downloads' })).toBeNull();
    expect(button).toHaveFocus();
  });
});
