import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { RunSummary } from '../../lib/contracts';
import { RunStatus } from './RunStatus';

const base: RunSummary = {
  runId: 'run-1',
  kind: 'lifecycle_test',
  instanceId: null,
  state: 'running',
  startedAt: '2026-10-16T12:00:00Z',
  endedAt: null,
  exitCode: null,
  durationMs: null,
  drainTimedOut: false,
  outputTail: [],
  outputLinesDropped: 0,
  loggingDegraded: false,
  error: null,
  signal: null,
  diagnosis: null,
  server: null,
};

describe('RunStatus', () => {
  it('renders nothing without runs', () => {
    const { container } = render(<RunStatus current={null} last={null} />);
    expect(container).toBeEmptyDOMElement();
  });

  it('offers Stop for the running process', () => {
    const onStop = vi.fn();
    render(<RunStatus current={base} last={null} onStop={onStop} />);
    expect(screen.getByRole('region', { name: /Running: Lifecycle test child/ })).toHaveTextContent(
      'Running',
    );
    fireEvent.click(screen.getByRole('button', { name: 'Stop' }));
    expect(onStop).toHaveBeenCalledWith('run-1');
  });

  it('asks before stopping a game and explains unsaved progress', () => {
    const onStop = vi.fn();
    render(<RunStatus current={{ ...base, kind: 'game' }} last={null} onStop={onStop} />);
    fireEvent.click(screen.getByRole('button', { name: 'Stop game' }));
    expect(onStop).not.toHaveBeenCalled();
    expect(screen.getByText(/not saved in the current world is lost/)).toBeVisible();
    fireEvent.click(screen.getByRole('button', { name: 'Keep playing' }));
    expect(onStop).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Stop game' }));
    fireEvent.click(screen.getByRole('button', { name: 'Stop without saving' }));
    expect(onStop).toHaveBeenCalledWith('run-1');
  });

  it('asks in place and gives the focus back to Stop game', () => {
    const onStop = vi.fn();
    render(
      <RunStatus
        current={{ ...base, kind: 'game', instanceId: 'i-1' }}
        last={null}
        names={new Map([['i-1', 'Survival']])}
        onStop={onStop}
      />,
    );
    const region = screen.getByRole('region', { name: /Running: Minecraft/ });
    expect(region).toHaveTextContent('Minecraft, “Survival”');
    const stop = screen.getByRole('button', { name: 'Stop game' });
    stop.focus();
    fireEvent.click(stop);
    const question = screen.getByRole('alertdialog', { name: 'Stop game' });
    expect(question).toHaveFocus();
    fireEvent.keyDown(question, { key: 'Escape' });
    expect(onStop).not.toHaveBeenCalled();
    expect(screen.getByRole('button', { name: 'Stop game' })).toHaveFocus();
  });

  it('keeps the output folded unless asked to show it', () => {
    const last: RunSummary = { ...base, state: 'exited', durationMs: 61_400, outputTail: ['x'] };
    const { rerender } = render(<RunStatus current={null} last={last} />);
    expect(screen.getByText('1 min 1 s')).toBeVisible();
    expect(screen.getByText('x').closest('details')).not.toBeNull();
    rerender(<RunStatus current={null} last={last} level={3} output="shown" />);
    expect(screen.getByRole('heading', { level: 3, name: 'Last run' })).toBeVisible();
    expect(screen.getByRole('region', { name: 'Last output lines' })).toHaveTextContent('x');
  });

  it('labels a run that failed before starting', () => {
    render(<RunStatus current={null} last={{ ...base, kind: 'game', state: 'failed' }} />);
    expect(screen.getByText('Not started')).toBeVisible();
  });

  it('shows a game ended by Stop as stopped, not as an error', () => {
    render(<RunStatus current={null} last={{ ...base, kind: 'game', state: 'stopped' }} />);
    expect(screen.getByText('Stopped by you')).toBeVisible();
    expect(screen.queryByText('Exited with an error')).toBeNull();
  });

  it('reports degraded logging', () => {
    render(<RunStatus current={null} last={{ ...base, state: 'exited', loggingDegraded: true }} />);
    expect(screen.getByText(/could not be written to disk/)).toBeVisible();
  });

  it('shows a crash result with exit code, drain notice and output tail', () => {
    const last: RunSummary = {
      ...base,
      state: 'failed',
      exitCode: 3,
      durationMs: 4200,
      drainTimedOut: true,
      outputTail: ['tick 1', 'test child: simulated crash'],
      outputLinesDropped: 12,
    };
    render(<RunStatus current={null} last={last} />);
    const region = screen.getByRole('region', { name: /Last run/ });
    expect(region).toHaveTextContent('Exited with an error');
    expect(region).toHaveTextContent('3');
    expect(region).toHaveTextContent('4.2 s');
    expect(region).toHaveTextContent('Output capture was closed 2 seconds after exit');
    expect(region).toHaveTextContent('12 earlier lines not shown.');
    expect(region).toHaveTextContent('simulated crash');
  });

  it('names the cause of a diagnosed crash and the signal', () => {
    const last: RunSummary = {
      ...base,
      kind: 'game',
      state: 'failed',
      durationMs: 1000,
      signal: 9,
      error: {
        code: 'GAME_EXITED_WITH_ERROR',
        message: 'The game exited with an error.',
        retryable: false,
        diagnosticId: 'd-1',
      },
      diagnosis: {
        kind: 'killed',
        details: { signal: '9' },
        evidence: [],
        suspects: [],
        report: null,
        actions: ['memory_settings'],
        notes: [],
      },
    };
    render(<RunStatus current={null} last={last} />);
    const region = screen.getByRole('region', { name: /Last run/ });
    expect(region).toHaveTextContent('The game stopped: The game was killed.');
    expect(region).toHaveTextContent('SIG 9');
    expect(region).not.toHaveTextContent('The game exited with an error.');
  });
});
