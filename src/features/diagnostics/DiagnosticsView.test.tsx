import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it } from 'vitest';
import { instanceFixture, snapshotFixture } from '../../test/fixtures';
import { DiagnosticsView } from './DiagnosticsView';

function backend(saved: boolean) {
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === 'export_diagnostics') {
      return saved
        ? { saved: true, fileName: 'kurasis-diagnostics.zip' }
        : { saved: false, fileName: null };
    }
    if (cmd === 'get_app_snapshot') return snapshotFixture();
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

describe('DiagnosticsView', () => {
  it('lists recent operations with their outcome', () => {
    backend(true);
    render(<DiagnosticsView snapshot={snapshot} />);
    const row = screen.getByRole('row', { name: /Repair Survival Failed/ });
    expect(row).toHaveTextContent('A download source could not be reached.');
    expect(row).toHaveTextContent('2.0 KiB');
  });

  it('explains the export and reports where it was saved', async () => {
    const calls = backend(true);
    render(<DiagnosticsView snapshot={snapshot} />);
    expect(screen.getByText(/Never included: account records, sign-in tokens/)).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Export…' }));
    expect(await screen.findByText(/Saved kurasis-diagnostics.zip/)).toBeVisible();
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
