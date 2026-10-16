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
  error: null,
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
});
