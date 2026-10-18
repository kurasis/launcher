import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it } from 'vitest';
import type { FailureSummary, ReportPreview } from '../../lib/contracts';
import { snapshotFixture } from '../../test/fixtures';
import { PrivacySettings } from '../settings/PrivacySettings';
import { ExplanationFeedback } from './ExplanationFeedback';
import { FailureOffer } from './FailureOffer';
import { ReportDialog } from './ReportDialog';

const preview: ReportPreview = {
  id: 'p-1',
  source: 'game',
  kind: 'out_of_memory',
  signature: 'out_of_memory:memory=2048',
  game: { version: '1.21.1', loader: 'fabric', loaderVersion: '0.18.2', java: '21', mods: 40 },
  files: [
    { name: 'output.log', text: 'Setting user: <player>\njava.lang.OutOfMemoryError' },
    { name: 'diagnosis.json', text: '{"kind":"out_of_memory"}' },
  ],
  removed: { chatLines: 2, names: 1, addresses: 0, worlds: 1, ids: 0 },
};

type Call = { cmd: string; args: Record<string, unknown> };

function mock(answers: Record<string, unknown>) {
  const calls: Call[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: args as Record<string, unknown> });
    return cmd in answers ? answers[cmd] : null;
  });
  return calls;
}

describe('ReportDialog', () => {
  it('shows the scrubbed files first and sends only the chosen ones with the note', async () => {
    const calls = mock({ preview_crash_report: preview, send_crash_report: 'sent' });
    render(
      <ReportDialog
        subject={{ type: 'game', runId: 'r-1', instanceId: 'i-1' }}
        onClose={() => undefined}
      />,
    );
    expect(
      await screen.findByText('Removed: 2 chat lines, 1 names, 0 addresses, 1 world names, 0 IDs.'),
    ).toBeVisible();
    expect(calls[0]).toMatchObject({
      cmd: 'preview_crash_report',
      args: { subject: { type: 'game', runId: 'r-1', instanceId: 'i-1' } },
    });
    expect(screen.getByText(/OutOfMemoryError/)).toBeInTheDocument();
    expect(calls.some((c) => c.cmd === 'send_crash_report')).toBe(false);

    fireEvent.click(screen.getByRole('checkbox', { name: 'diagnosis.json' }));
    fireEvent.change(screen.getByLabelText('What were you doing? (optional)'), {
      target: { value: '  Opened a world  ' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));
    expect(await screen.findByText('Thank you! The report was sent.')).toBeVisible();
    expect(calls.find((c) => c.cmd === 'send_crash_report')?.args).toEqual({
      previewId: 'p-1',
      files: ['output.log'],
      note: 'Opened a world',
    });
  });

  it('cannot send without files and says when the report waits for the service', async () => {
    mock({ preview_crash_report: preview, send_crash_report: 'queued' });
    render(
      <ReportDialog subject={{ type: 'launcher', failureId: 'f-1' }} onClose={() => undefined} />,
    );
    await screen.findByRole('checkbox', { name: 'output.log' });
    fireEvent.click(screen.getByRole('checkbox', { name: 'output.log' }));
    fireEvent.click(screen.getByRole('checkbox', { name: 'diagnosis.json' }));
    expect(screen.getByRole('button', { name: 'Send' })).toBeDisabled();
    fireEvent.click(screen.getByRole('checkbox', { name: 'output.log' }));
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));
    expect(await screen.findByText(/The report is kept and sent by itself later/)).toBeVisible();
  });
});

describe('FailureOffer', () => {
  const failure = (overrides: Partial<FailureSummary>): FailureSummary => ({
    id: 'f-1',
    kind: 'panic',
    at: new Date().toISOString(),
    version: '0.1.0',
    message: 'boom',
    reported: false,
    ...overrides,
  });

  it('offers the newest unreported recent failure and forgets it when declined', async () => {
    let failures = [
      failure({ id: 'old', at: '2020-01-01T00:00:00Z', kind: 'frontend' }),
      failure({ id: 'f-1', kind: 'unclean_exit' }),
    ].reverse();
    const calls: Call[] = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args: args as Record<string, unknown> });
      if (cmd === 'list_launcher_failures') return failures;
      if (cmd === 'dismiss_launcher_failure') {
        failures = failures.map((f) => ({ ...f, reported: true }));
      }
      return null;
    });
    render(<FailureOffer />);
    expect(await screen.findByText('The launcher did not close properly last time.')).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: "Don't send" }));
    await waitFor(() => {
      expect(screen.queryByText('The launcher had a problem')).toBeNull();
    });
    expect(calls.find((c) => c.cmd === 'dismiss_launcher_failure')?.args).toEqual({ id: 'f-1' });
  });

  it('shows nothing when every failure is old or handled', async () => {
    const calls = mock({
      list_launcher_failures: [
        failure({ reported: true }),
        failure({ id: 'old', at: '2020-01-01T00:00:00Z' }),
      ],
    });
    const { container } = render(<FailureOffer />);
    await waitFor(() => {
      expect(calls.length).toBe(1);
    });
    expect(container).toBeEmptyDOMElement();
  });
});

describe('ExplanationFeedback', () => {
  it('sends the kind, signature and answer once', async () => {
    const calls = mock({});
    render(
      <ExplanationFeedback
        diagnosis={{
          kind: 'out_of_memory',
          details: {},
          evidence: [],
          suspects: [],
          report: null,
          actions: [],
          notes: [],
          signature: 'out_of_memory:memory=2048',
        }}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'No' }));
    expect(await screen.findByText('Thank you for the answer.')).toBeVisible();
    expect(calls.find((c) => c.cmd === 'send_explanation_feedback')?.args).toEqual({
      kind: 'out_of_memory',
      signature: 'out_of_memory:memory=2048',
      helpful: false,
    });
    expect(screen.queryByRole('button', { name: 'Yes' })).toBeNull();
  });
});

describe('PrivacySettings', () => {
  it('turns statistics on and makes a new install ID', async () => {
    const calls = mock({ get_app_snapshot: snapshotFixture() });
    const base = snapshotFixture().settings;
    const { rerender } = render(<PrivacySettings settings={base} />);
    expect(screen.getByText('Not created yet: it appears with your first report.')).toBeVisible();
    const toggle = screen.getByRole('checkbox', { name: /Share anonymous crash statistics/ });
    expect(toggle).not.toBeChecked();
    fireEvent.click(toggle);
    await waitFor(() => {
      expect(calls.find((c) => c.cmd === 'set_app_settings')?.args).toMatchObject({
        request: { payload: { shareStats: true } },
      });
    });
    rerender(
      <PrivacySettings
        settings={{ ...base, shareStats: true, installId: '9b2f1c3e-0000-4000-8000-000000000001' }}
      />,
    );
    expect(screen.getByText('9b2f1c3e-0000-4000-8000-000000000001')).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Make a new ID' }));
    await waitFor(() => {
      expect(calls.filter((c) => c.cmd === 'set_app_settings')[1]?.args).toMatchObject({
        request: { payload: { newInstallId: true } },
      });
    });
  });
});
