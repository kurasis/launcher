import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { describe, expect, it } from 'vitest';
import type { ConsoleEntry, ConsolePage } from '../../lib/contracts';
import { ConsoleApp } from './ConsoleApp';

const at = '2026-10-17T12:00:00.000Z';
const entries: ConsoleEntry[] = [
  { seq: 4, at, source: 'launcher', level: 'info', text: 'Launcher 0.1.0 started' },
  { seq: 5, at, source: 'game', level: 'warn', text: '[main/WARN]: Low memory' },
  {
    seq: 6,
    at,
    source: 'game',
    level: 'error',
    stderr: true,
    text: '[Render thread/ERROR]: crash',
  },
];

describe('ConsoleApp', () => {
  it('polls the journal from where it stopped and filters lines', async () => {
    const reads: number[] = [];
    let delivered = false;
    mockIPC((cmd, args) => {
      if (cmd !== 'read_console') return null;
      const { after } = args as { after: number };
      reads.push(after);
      const page: ConsolePage = delivered
        ? { entries: [], lastSeq: 6, missed: 0 }
        : { entries, lastSeq: 6, missed: 3 };
      delivered = true;
      return page;
    });
    render(<ConsoleApp />);
    expect(await screen.findByText('Launcher 0.1.0 started')).toBeVisible();
    expect(
      screen.getByText('3 older lines were dropped (the console keeps the newest 10,000).'),
    ).toBeVisible();
    await waitFor(() => {
      expect(reads.slice(0, 2)).toEqual([0, 6]);
    });

    fireEvent.click(screen.getByRole('button', { name: 'Game' }));
    expect(screen.queryByText('Launcher 0.1.0 started')).not.toBeInTheDocument();
    expect(screen.getByText('2 of 3 lines')).toBeVisible();

    fireEvent.change(screen.getByRole('combobox', { name: 'All levels' }), {
      target: { value: 'error' },
    });
    expect(screen.queryByText('[main/WARN]: Low memory')).not.toBeInTheDocument();
    expect(screen.getByText('[Render thread/ERROR]: crash')).toBeVisible();

    fireEvent.click(screen.getByRole('button', { name: 'All' }));
    fireEvent.change(screen.getByRole('combobox', { name: 'All levels' }), {
      target: { value: 'all' },
    });
    fireEvent.change(screen.getByRole('searchbox', { name: 'Filter lines' }), {
      target: { value: 'memory' },
    });
    expect(screen.getByText('1 of 3 lines')).toBeVisible();

    fireEvent.click(screen.getByRole('button', { name: 'Clear' }));
    expect(screen.getByText(/^Nothing yet/)).toBeVisible();
  });

  it('says when the core does not answer', async () => {
    mockIPC(() => {
      throw new Error('gone');
    });
    render(<ConsoleApp />);
    expect(await screen.findByRole('alert')).toHaveTextContent('does not answer');
  });
});
