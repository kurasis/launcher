import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it } from 'vitest';
import type { ApprovalRequest, OperationProgress } from '../../lib/contracts';
import { instanceFixture, snapshotFixture } from '../../test/fixtures';
import { AsideApproval } from './AsideApproval';
import { MeteredApproval } from './MeteredApproval';

const survival = instanceFixture({ displayName: 'Survival' });
const MIB = 1024 * 1024;

const large: ApprovalRequest = {
  bytes: 550 * MIB,
  files: 3955,
  reason: 'large',
  monthBytes: 900 * MIB,
  monthlyLimitMib: 1024,
};

function asking(operationId: string, approval: ApprovalRequest, instanceId = survival.id) {
  const progress: OperationProgress = {
    sessionEpoch: 'epoch-1',
    operationId,
    instanceId,
    kind: 'install',
    sequence: 1,
    revision: 1,
    phase: 'awaiting_approval',
    downloadedBytes: 0,
    totalDownloadBytes: null,
    verifiedFiles: 0,
    totalFiles: null,
    bytesPerSecond: null,
    currentFiles: [],
    approval,
    timestamp: '2026-10-23T00:00:00Z',
  };
  return snapshotFixture({
    instances: [survival],
    activeOperation: { kind: 'install', instanceId, operationId },
    operationProgress: progress,
  });
}

function backend(answer: () => unknown = () => null) {
  const answers: { operationId: string; allow: boolean }[] = [];
  mockIPC((cmd, args) => {
    if (cmd === 'answer_download') {
      const { request } = args as { request: { payload: { operationId: string; allow: boolean } } };
      answers.push(request.payload);
      return answer();
    }
    return null;
  });
  return answers;
}

const plain = (text: string | null) => (text ?? '').replace(/[⁨⁩]/g, '');

describe('the question before a download on a metered connection', () => {
  it('asks by itself, with the files, the installation and the month', async () => {
    const answers = backend();
    render(<MeteredApproval snapshot={asking('op-a', large)} />);
    const dialog = screen.getByRole('dialog', { name: 'Download 550.0 MiB?' });
    expect(plain(dialog.textContent)).toContain(
      '3,955 files for “Survival” will be downloaded over a metered connection. Used this month: 900.0 MiB of 1.0 GiB.',
    );
    expect(dialog).toHaveAccessibleDescription(/^3,955 files for .+ 1\.0 GiB\.$/);
    // It came by itself: the dialog holds the focus, not an answer.
    await waitFor(() => {
      expect(dialog).toHaveFocus();
    });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Download' }));
    await waitFor(() => {
      expect(answers).toEqual([{ operationId: 'op-a', allow: true }]);
    });
  });

  it('declines with Don’t download', async () => {
    const answers = backend();
    render(<MeteredApproval snapshot={asking('op-b', { ...large, monthlyLimitMib: 0 })} />);
    const dialog = screen.getByRole('dialog', { name: 'Download 550.0 MiB?' });
    expect(plain(dialog.textContent)).toContain('Used this month: 900.0 MiB.');
    fireEvent.click(within(dialog).getByRole('button', { name: 'Don’t download' }));
    await waitFor(() => {
      expect(answers).toEqual([{ operationId: 'op-b', allow: false }]);
    });
  });

  it('says how a download would pass the monthly limit', () => {
    backend();
    render(
      <MeteredApproval
        snapshot={asking('op-c', { ...large, reason: 'over_limit', monthlyLimitMib: 1024 })}
      />,
    );
    expect(
      screen.getByText(
        'This download of 550.0 MiB would pass your monthly limit: 900.0 MiB of 1.0 GiB used so far.',
      ),
    ).toBeVisible();
  });

  it('names the size when the installation is not known', () => {
    backend();
    render(<MeteredApproval snapshot={asking('op-d', large, 'gone')} />);
    expect(
      screen.getByText(
        '3,955 files (550.0 MiB) will be downloaded over a metered connection. Used this month: 900.0 MiB of 1.0 GiB.',
      ),
    ).toBeVisible();
  });

  it('answers once, and stays while the answer is sent', async () => {
    let finish: (value: null) => void = () => undefined;
    const answers = backend(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    render(<MeteredApproval snapshot={asking('op-e', large)} />);
    const dialog = screen.getByRole('dialog');
    const download = within(dialog).getByRole('button', { name: 'Download' });
    fireEvent.click(download);
    await waitFor(() => {
      expect(download).toHaveAttribute('aria-disabled', 'true');
    });
    fireEvent.click(download);
    fireEvent.click(within(dialog).getByRole('button', { name: 'Don’t download' }));
    fireEvent.keyDown(dialog, { key: 'Escape' });
    expect(screen.getByRole('dialog')).toBeVisible();
    await act(async () => {
      finish(null);
      await Promise.resolve();
    });
    expect(answers).toEqual([{ operationId: 'op-e', allow: true }]);
  });

  it('shows why an answer did not go', async () => {
    backend(() => {
      throw new Error('core away');
    });
    render(<MeteredApproval snapshot={asking('op-f', large)} />);
    fireEvent.click(screen.getByRole('button', { name: 'Download' }));
    expect(await within(screen.getByRole('dialog')).findByRole('alert')).toBeVisible();
  });

  it('waits unanswered when closed, until it is shown again', () => {
    const answers = backend();
    const snapshot = asking('op-g', large);
    render(
      <>
        <MeteredApproval snapshot={snapshot} />
        <AsideApproval operationId="op-g" />
      </>,
    );
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' });
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(answers).toEqual([]);
    const again = screen.getByRole('button', { name: 'Show the question' });
    again.focus();
    fireEvent.click(again);
    expect(screen.getByRole('dialog', { name: 'Download 550.0 MiB?' })).toBeVisible();
    // Closed again, the focus is back on the button that showed it.
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' });
    expect(again).toHaveFocus();
  });

  it('puts aside only the question it closed', () => {
    backend();
    const { rerender } = render(<MeteredApproval snapshot={asking('op-h', large)} />);
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' });
    expect(screen.queryByRole('dialog')).toBeNull();
    rerender(<MeteredApproval snapshot={asking('op-i', large)} />);
    expect(screen.getByRole('dialog', { name: 'Download 550.0 MiB?' })).toBeVisible();
  });

  it('takes the focus back itself, never onto an answer', async () => {
    backend();
    const snapshot = asking('op-j', large);
    const { rerender } = render(<MeteredApproval snapshot={snapshot} />);
    const dialog = screen.getByRole('dialog');
    await waitFor(() => {
      expect(dialog).toHaveFocus();
    });
    // The focus fell to the page (the window lost it, a control went away).
    dialog.blur();
    rerender(<MeteredApproval snapshot={{ ...snapshot }} />);
    expect(dialog).toHaveFocus();
  });

  it('asks nothing when no download waits', () => {
    backend();
    render(<MeteredApproval snapshot={snapshotFixture()} />);
    expect(screen.queryByRole('dialog')).toBeNull();
  });
});
