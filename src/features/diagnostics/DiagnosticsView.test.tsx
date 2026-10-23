import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { RunSummary } from '../../lib/contracts';
import { appInfoFixture, instanceFixture, snapshotFixture } from '../../test/fixtures';
import { DiagnosticsView } from './DiagnosticsView';

function backend(saved: boolean) {
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === 'export_diagnostics') {
      return saved
        ? { saved: true, fileName: 'launcher-diagnostics.zip' }
        : { saved: false, fileName: null };
    }
    if (cmd === 'get_app_snapshot') return snapshotFixture();
    if (cmd === 'get_app_info') return appInfoFixture;
    if (cmd === 'open_console') throw new Error('The console window could not open.');
    return null;
  });
  return calls;
}

const snapshot = snapshotFixture({
  instances: [instanceFixture()],
  recentOperations: [
    {
      operationId: 'op-1',
      instanceId: instanceFixture().id,
      kind: 'repair',
      outcome: 'failed',
      error: {
        code: 'NETWORK_ERROR',
        message: 'A download source could not be reached.',
        retryable: true,
        diagnosticId: 'd',
      },
      finishedAt: '2026-10-17T00:00:00Z',
      downloadedFiles: 0,
      networkBytes: 2048,
      reusedFiles: 10,
    },
  ],
});

const lastRun: RunSummary = {
  runId: 'run-7',
  kind: 'game',
  instanceId: instanceFixture().id,
  state: 'exited',
  startedAt: '2026-10-16T12:00:00Z',
  endedAt: '2026-10-16T12:42:10Z',
  exitCode: 0,
  durationMs: 2_530_000,
  drainTimedOut: false,
  outputTail: ['[Render thread/INFO]: Stopping!', '<b>not markup</b>'],
  outputLinesDropped: 0,
  loggingDegraded: false,
  error: null,
  signal: null,
  diagnosis: null,
  server: null,
};

describe('DiagnosticsView', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it('heads the screen and lists recent operations with their outcome and time', async () => {
    vi.useFakeTimers({ now: new Date(2026, 9, 17, 20, 0), shouldAdvanceTime: true });
    backend(true);
    const local = (day: number, hour: number) => new Date(2026, 9, day, hour, 5).toISOString();
    render(
      <DiagnosticsView
        snapshot={{
          ...snapshot,
          recentOperations: [
            ...snapshot.recentOperations.map((op) => ({ ...op, finishedAt: local(16, 19) })),
            {
              operationId: 'op-2',
              instanceId: null,
              kind: 'install',
              outcome: 'succeeded',
              error: null,
              finishedAt: local(3, 10),
              downloadedFiles: 0,
              networkBytes: 0,
              reusedFiles: 0,
            },
          ],
        }}
      />,
    );
    expect(screen.getByRole('heading', { level: 2, name: 'Diagnostics' })).toBeVisible();
    expect(screen.getAllByRole('columnheader').map((cell) => cell.textContent)).toEqual([
      'Operation',
      'Installation',
      'Result',
      'Downloaded',
      'When',
    ]);
    const row = screen.getByRole('row', { name: /Repair Survival Failed/ });
    expect(row).toHaveTextContent('A download source could not be reached.');
    expect(row).toHaveTextContent('2.0 KiB');
    expect(row).toHaveTextContent('yesterday, 7:05 PM');
    // Nothing downloaded leaves the cell empty; an older day reads as its date.
    const older = screen.getByRole('row', { name: /^Install — Succeeded/ });
    expect(
      within(older)
        .getAllByRole('cell')
        .map((cell) => cell.textContent),
    ).toEqual(['Install', '—', 'Succeeded', '', 'October 3']);
    expect(await screen.findByText('linux / x86_64')).toBeVisible();
  });

  it('shows the install under way first, but not a game run', () => {
    backend(true);
    const id = instanceFixture().id;
    const { rerender } = render(
      <DiagnosticsView
        snapshot={{
          ...snapshot,
          activeOperation: { kind: 'install', instanceId: id, operationId: 'op-9' },
          operationProgress: {
            sessionEpoch: 's',
            operationId: 'op-9',
            instanceId: id,
            kind: 'install',
            sequence: 1,
            revision: 1,
            phase: 'downloading',
            downloadedBytes: 3 * 1024 * 1024,
            totalDownloadBytes: null,
            verifiedFiles: 0,
            totalFiles: null,
            bytesPerSecond: null,
            currentFiles: [],
            approval: null,
            timestamp: '2026-10-17T00:00:00Z',
          },
        }}
      />,
    );
    const rows = screen.getAllByRole('row');
    expect(rows[1]).toHaveTextContent(/^InstallSurvivalIn progress3\.0 MiBnow$/);
    rerender(
      <DiagnosticsView
        snapshot={{
          ...snapshot,
          activeOperation: { kind: 'launch', instanceId: id, operationId: 'run-1' },
        }}
      />,
    );
    expect(screen.queryByText('In progress')).toBeNull();
  });

  it('opens the console from its row and says why it could not', async () => {
    const calls = backend(true);
    render(<DiagnosticsView snapshot={snapshot} />);
    const open = screen.getByRole('button', { name: 'Open console' });
    expect(within(screen.getByRole('region', { name: 'Console' })).getByRole('button')).toBe(open);
    fireEvent.click(open);
    expect(await screen.findByText(/could not open/)).toBeVisible();
    expect(calls.some((c) => c.cmd === 'open_console')).toBe(true);
  });

  it('shows the last run with its output, or says there was none', async () => {
    backend(true);
    const plain = { ...snapshot, lifecycleHarnessAvailable: false };
    const { rerender } = render(<DiagnosticsView snapshot={{ ...plain, lastRun }} />);
    const last = screen.getByRole('region', { name: 'Last run' });
    expect(last).toHaveTextContent('Minecraft, “Survival”');
    expect(last).toHaveTextContent('Exited normally');
    expect(last).toHaveTextContent('42 min 10 s');
    const output = within(last).getByRole('region', { name: 'Last output lines' });
    expect(output.tagName).toBe('PRE');
    expect(output).toHaveAttribute('dir', 'ltr');
    expect(output).toHaveAttribute('tabindex', '0');
    expect(output.textContent).toContain('<b>not markup</b>');
    expect(output.querySelector('b')).toBeNull();
    rerender(<DiagnosticsView snapshot={{ ...plain, lastRun: null, currentRun: null }} />);
    expect(screen.getByRole('region', { name: 'Last run' })).toHaveTextContent(
      'No game has run since the launcher started.',
    );
    // The computer's memory is told with the core's facts.
    expect(await screen.findByText('Memory')).toBeVisible();
  });

  it('explains the export and reports where it was saved', async () => {
    const calls = backend(true);
    render(<DiagnosticsView snapshot={snapshot} />);
    expect(screen.getByText(/Never included: account records, sign-in tokens/)).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Export…' }));
    expect(await screen.findByText(/Saved launcher-diagnostics.zip/)).toBeVisible();
    expect(calls.find((c) => c.cmd === 'export_diagnostics')?.args).toMatchObject({
      request: { payload: { runId: null } },
    });
  });

  it('says so when the save dialog is closed', async () => {
    backend(false);
    render(<DiagnosticsView snapshot={snapshot} />);
    fireEvent.click(screen.getByRole('button', { name: 'Export…' }));
    await waitFor(() => {
      expect(screen.getByText('Export cancelled.')).toBeVisible();
    });
  });
});
