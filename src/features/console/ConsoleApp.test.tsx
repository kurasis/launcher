import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { mockIPC } from '@tauri-apps/api/mocks';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { ConsoleEntry, ConsolePage } from '../../lib/contracts';
import { THEME_STORAGE_KEY } from '../../lib/theme';
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

/** The lines once, then nothing new. */
function firstPage(after: number, missed: number): ConsolePage {
  return after === 0 ? { entries, lastSeq: 6, missed } : { entries: [], lastSeq: 6, missed: 0 };
}

afterEach(() => {
  window.localStorage.removeItem(THEME_STORAGE_KEY);
  delete document.documentElement.dataset.theme;
});

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

    fireEvent.click(screen.getByRole('radio', { name: 'Game' }));
    expect(screen.queryByText('Launcher 0.1.0 started')).not.toBeInTheDocument();
    expect(screen.getByText('2 of 3 lines')).toBeVisible();

    fireEvent.change(screen.getByRole('combobox', { name: 'Show levels' }), {
      target: { value: 'error' },
    });
    expect(screen.queryByText('[main/WARN]: Low memory')).not.toBeInTheDocument();
    expect(screen.getByText('[Render thread/ERROR]: crash')).toBeVisible();

    fireEvent.click(screen.getByRole('radio', { name: 'All' }));
    fireEvent.change(screen.getByRole('combobox', { name: 'Show levels' }), {
      target: { value: 'all' },
    });
    fireEvent.change(screen.getByRole('searchbox', { name: 'Filter lines' }), {
      target: { value: 'memory' },
    });
    expect(screen.getByText('1 of 3 lines')).toBeVisible();

    fireEvent.click(screen.getByRole('button', { name: 'Clear' }));
    expect(screen.getByText(/^Nothing yet/)).toBeVisible();
  });

  it('names the log, tags lines in sentence case and copies them with the plain-text tags', async () => {
    mockIPC((cmd, args) =>
      cmd === 'read_console' ? firstPage((args as { after: number }).after, 2) : null,
    );
    const writeText = vi.fn(() => Promise.resolve());
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    render(<ConsoleApp />);
    const log = await screen.findByRole('log', { name: 'Launcher and game output' });
    const first = (await within(log).findByText('Launcher 0.1.0 started')).parentElement;
    expect(first).not.toBeNull();
    expect(within(first as HTMLElement).getByText('Launcher')).toBeVisible();
    expect(screen.getByText('The console keeps the newest 10,000 lines')).toBeVisible();
    // The dropped count grows with each read: it is shown, not read out every time.
    const dropped = screen.getByText(/older lines were dropped/);
    expect(dropped.closest('[role]')).toBeNull();

    fireEvent.click(screen.getByRole('radio', { name: 'Game' }));
    fireEvent.click(screen.getByRole('button', { name: 'Copy' }));
    expect(await screen.findByRole('button', { name: 'Copied' })).toBeVisible();
    const time = within(log).getAllByText(/^\d\d:\d\d:\d\d\.\d{3}$/)[0]?.textContent ?? '';
    expect(writeText).toHaveBeenCalledWith(
      `${time} GAME [main/WARN]: Low memory\n${time} GAME [Render thread/ERROR]: crash`,
    );
  });

  it('bounds the filter text', async () => {
    mockIPC((cmd, args) =>
      cmd === 'read_console' ? firstPage((args as { after: number }).after, 0) : null,
    );
    render(<ConsoleApp />);
    const search = await screen.findByRole('searchbox', { name: 'Filter lines' });
    expect(search).toHaveAttribute('maxlength', '200');
    fireEvent.change(search, { target: { value: 'x'.repeat(500) } });
    expect(search).toHaveValue('x'.repeat(200));
  });

  it('follows the theme the launcher keeps for it', async () => {
    mockIPC((cmd) =>
      cmd === 'read_console'
        ? ({ entries: [], lastSeq: 0, missed: 0 } satisfies ConsolePage)
        : null,
    );
    // Kept by the main window after this one's first paint: taken when it starts listening.
    window.localStorage.setItem(THEME_STORAGE_KEY, 'light');
    render(<ConsoleApp />);
    await screen.findByText(/^Nothing yet/);
    expect(document.documentElement.dataset.theme).toBe('light');
    act(() => {
      window.localStorage.setItem(THEME_STORAGE_KEY, 'dark');
      window.dispatchEvent(new StorageEvent('storage', { key: THEME_STORAGE_KEY }));
    });
    expect(document.documentElement.dataset.theme).toBe('dark');
    act(() => {
      window.localStorage.setItem(THEME_STORAGE_KEY, 'system');
      window.dispatchEvent(new StorageEvent('storage', { key: THEME_STORAGE_KEY }));
    });
    expect(document.documentElement.dataset.theme).toBeUndefined();
  });

  it('says when the core does not answer', async () => {
    mockIPC(() => {
      throw new Error('gone');
    });
    render(<ConsoleApp />);
    expect(await screen.findByRole('alert')).toHaveTextContent('does not answer');
  });
});
